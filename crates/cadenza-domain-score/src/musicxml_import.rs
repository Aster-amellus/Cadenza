use crate::model::{
    Hand, PlaybackMidiEvent, Score, ScoreMeta, ScoreSource, TargetEvent, TempoPoint, Track,
};
use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::types::Tick;
use roxmltree::Document;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

#[derive(thiserror::Error, Debug)]
pub enum MusicXmlImportError {
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unsupported feature: {0}")]
    Unsupported(String),
}

#[derive(Clone, Debug)]
struct NoteEvent {
    tick: Tick,
    duration_ticks: Tick,
    note: u8,
    velocity: u8,
    hand: Option<Hand>,
    measure_index: Option<u32>,
}

type TargetGroup = (Vec<(u8, Option<Hand>)>, Option<u32>);

pub fn import_musicxml_path(path: &Path) -> Result<Score, MusicXmlImportError> {
    let data = read_musicxml_file(path)?;
    import_musicxml_str(&data)
}

pub fn import_musicxml_str(xml: &str) -> Result<Score, MusicXmlImportError> {
    let doc = Document::parse(xml).map_err(|e| MusicXmlImportError::Parse(e.to_string()))?;
    let title = doc
        .descendants()
        .find(|node| node.has_tag_name("work-title"))
        .and_then(|node| node.text())
        .map(|text| text.to_string());

    let ppq: u16 = 480;
    let mut tempo_points: BTreeMap<Tick, u32> = BTreeMap::new();
    let mut note_events: Vec<NoteEvent> = Vec::new();
    let mut cc64_events: Vec<PlaybackMidiEvent> = Vec::new();

    for part in doc.descendants().filter(|node| node.has_tag_name("part")) {
        let mut current_tick: Tick = 0;
        let mut divisions: i64 = 1;
        let mut current_velocity: u8 = 90;
        let mut pedal_down = false;
        let mut time_beats: i64 = 4;
        let mut time_beat_type: i64 = 4;
        let mut measure_index: u32 = 0;
        let mut active_ties: HashMap<(u8, Option<Hand>), usize> = HashMap::new();
        let mut max_note_end_tick: Tick = 0;

        for measure in part
            .children()
            .filter(|node| node.is_element() && node.has_tag_name("measure"))
        {
            let measure_is_implicit = measure
                .attribute("implicit")
                .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "yes" | "true"));
            let measure_start = current_tick.max(0);
            let mut cursor = measure_start;
            let mut measure_end = measure_start;

            let mut last_note_start_tick: Option<Tick> = None;
            let measure_len_ticks = measure_length_ticks(ppq, time_beats, time_beat_type);
            let mut expected_end_tick = if measure_len_ticks > 0 {
                Some(measure_start.saturating_add(measure_len_ticks))
            } else {
                None
            };

            for element in measure.children().filter(|node| node.is_element()) {
                if element.has_tag_name("attributes") {
                    if let Some(div_node) = element
                        .children()
                        .find(|node| node.has_tag_name("divisions"))
                    {
                        if let Some(text) = div_node.text() {
                            divisions = text.parse::<i64>().unwrap_or(1).max(1);
                        }
                    }
                    if let Some(time_node) =
                        element.children().find(|node| node.has_tag_name("time"))
                    {
                        if let (Some(beats), Some(beat_type)) = (
                            time_node
                                .children()
                                .find(|node| node.has_tag_name("beats"))
                                .and_then(|node| node.text())
                                .and_then(parse_beats),
                            time_node
                                .children()
                                .find(|node| node.has_tag_name("beat-type"))
                                .and_then(|node| node.text())
                                .and_then(|t| t.trim().parse::<i64>().ok()),
                        ) {
                            if beats > 0 && beat_type > 0 {
                                time_beats = beats;
                                time_beat_type = beat_type;
                                let measure_len_ticks =
                                    measure_length_ticks(ppq, time_beats, time_beat_type);
                                expected_end_tick = if measure_len_ticks > 0 {
                                    Some(measure_start.saturating_add(measure_len_ticks))
                                } else {
                                    None
                                };
                            }
                        }
                    }
                } else if element.has_tag_name("direction") {
                    let tick = cursor.max(0);
                    if let Some(sound) = element.children().find(|node| node.has_tag_name("sound"))
                    {
                        if let Some(tempo_attr) = sound.attribute("tempo") {
                            if let Ok(bpm) = tempo_attr.parse::<f64>() {
                                if bpm > 0.0 {
                                    let us_per_quarter = (60_000_000.0 / bpm) as u32;
                                    tempo_points.insert(tick, us_per_quarter);
                                }
                            }
                        }

                        if let Some(value) = sound.attribute("dynamics") {
                            if let Some(vel) = parse_velocity(value) {
                                current_velocity = vel;
                            }
                        }

                        if let Some(value) = sound
                            .attribute("damper-pedal")
                            .or_else(|| sound.attribute("pedal"))
                        {
                            if let Some(down) = parse_pedal_value(value) {
                                emit_cc64_change(&mut cc64_events, tick, &mut pedal_down, down);
                            }
                        }
                    }

                    if let Some(direction_type) = element
                        .children()
                        .find(|node| node.is_element() && node.has_tag_name("direction-type"))
                    {
                        if let Some(vel) = parse_dynamics_mark(&direction_type)
                            .or_else(|| parse_dynamics_words(&direction_type))
                        {
                            current_velocity = vel;
                        }
                        for pedal_node in direction_type
                            .children()
                            .filter(|node| node.is_element() && node.has_tag_name("pedal"))
                        {
                            if let Some(down) = parse_pedal_mark(&pedal_node, pedal_down) {
                                emit_cc64_change(&mut cc64_events, tick, &mut pedal_down, down);
                            }
                        }

                        if let Some(down) = parse_pedal_words(&direction_type, pedal_down) {
                            emit_cc64_change(&mut cc64_events, tick, &mut pedal_down, down);
                        }
                    }
                } else if element.has_tag_name("backup") {
                    let duration = duration_ticks(&element, divisions, ppq).max(0);
                    cursor = cursor.saturating_sub(duration).max(measure_start);
                    last_note_start_tick = None;
                } else if element.has_tag_name("forward") {
                    let duration = duration_ticks(&element, divisions, ppq).max(0);
                    cursor = cursor.saturating_add(duration);
                    measure_end = measure_end.max(cursor);
                    last_note_start_tick = None;
                } else if element.has_tag_name("note") {
                    let is_chord = element.children().any(|node| node.has_tag_name("chord"));
                    let is_rest = element.children().any(|node| node.has_tag_name("rest"));
                    let is_grace = element.children().any(|node| node.has_tag_name("grace"));
                    if is_grace {
                        continue;
                    }

                    let mut raw_duration = duration_ticks(&element, divisions, ppq);
                    let mut duration_missing = raw_duration == 0;
                    if duration_missing {
                        if let Some(inferred) = infer_note_duration_ticks(&element, ppq) {
                            raw_duration = inferred;
                            duration_missing = false;
                        }
                    }
                    let base_tick = if is_chord {
                        last_note_start_tick.unwrap_or(cursor)
                    } else {
                        cursor
                    };
                    let mut duration = raw_duration.max(0);
                    let max_len = expected_end_tick.map(|end_tick| (end_tick - base_tick).max(0));
                    if let Some(max_len) = max_len {
                        duration = duration.min(max_len);
                    }
                    let duration_for_note = duration.max(1);

                    if !is_rest {
                        if let Some(note) = parse_note(&element) {
                            let hand = parse_hand(&element);
                            let (tie_start, tie_stop) = parse_ties(&element);
                            let key = (note, hand);

                            if tie_stop {
                                if let Some(&idx) = active_ties.get(&key) {
                                    note_events[idx].duration_ticks = note_events[idx]
                                        .duration_ticks
                                        .saturating_add(duration_for_note);
                                    max_note_end_tick = max_note_end_tick.max(
                                        note_events[idx]
                                            .tick
                                            .saturating_add(note_events[idx].duration_ticks),
                                    );
                                    if !tie_start {
                                        active_ties.remove(&key);
                                    }
                                } else {
                                    let idx = note_events.len();
                                    note_events.push(NoteEvent {
                                        tick: base_tick.max(0),
                                        duration_ticks: duration_for_note,
                                        note,
                                        velocity: current_velocity,
                                        hand,
                                        measure_index: Some(measure_index),
                                    });
                                    max_note_end_tick = max_note_end_tick
                                        .max(base_tick.saturating_add(duration_for_note));
                                    if tie_start {
                                        active_ties.insert(key, idx);
                                    }
                                }
                            } else {
                                let idx = note_events.len();
                                note_events.push(NoteEvent {
                                    tick: base_tick.max(0),
                                    duration_ticks: duration_for_note,
                                    note,
                                    velocity: current_velocity,
                                    hand,
                                    measure_index: Some(measure_index),
                                });
                                max_note_end_tick = max_note_end_tick
                                    .max(base_tick.saturating_add(duration_for_note));
                                if tie_start {
                                    active_ties.insert(key, idx);
                                }
                            }
                        }
                    }

                    if !is_chord {
                        last_note_start_tick = if is_rest {
                            None
                        } else {
                            Some(base_tick.max(0))
                        };
                        let mut advance = duration;
                        if advance == 0 && duration_missing {
                            if let Some(max_len) = max_len {
                                if max_len > 0 {
                                    advance = 1;
                                }
                            } else {
                                advance = 1;
                            }
                        }
                        cursor = cursor.saturating_add(advance);
                        measure_end = measure_end.max(cursor);
                    }
                }
            }

            if let Some(end_tick) = expected_end_tick {
                if !measure_is_implicit {
                    measure_end = measure_end.max(end_tick);
                }
            }

            current_tick = measure_end;
            measure_index = measure_index.saturating_add(1);
        }

        // Ensure pedal is released for this part at end-of-score.
        if pedal_down {
            let end_tick = max_note_end_tick.max(current_tick);
            emit_cc64_change(&mut cc64_events, end_tick, &mut pedal_down, false);
        }
    }

    let tempo_map = build_tempo_map(tempo_points);
    apply_rearticulation_gaps(&mut note_events);
    let playback_events = build_playback_events(&note_events, &cc64_events);
    let targets = build_targets(&note_events);

    let track = Track {
        id: 0,
        name: "Merged".to_string(),
        hand: None,
        targets,
        playback_events,
    };

    let score = Score {
        meta: ScoreMeta {
            title,
            source: ScoreSource::MusicXml,
        },
        ppq,
        tempo_map,
        tracks: vec![track],
    };

    Ok(score)
}

fn duration_ticks(node: &roxmltree::Node, divisions: i64, ppq: u16) -> Tick {
    let duration = node
        .children()
        .find(|child| child.has_tag_name("duration"))
        .and_then(|child| child.text())
        .and_then(|text| text.parse::<i64>().ok())
        .unwrap_or(0);

    if divisions <= 0 {
        return 0;
    }
    let ppq = ppq as i64;
    let ticks = (duration.saturating_mul(ppq) + divisions / 2) / divisions;
    if duration > 0 && ticks == 0 {
        1
    } else {
        ticks
    }
}

fn infer_note_duration_ticks(node: &roxmltree::Node, ppq: u16) -> Option<Tick> {
    let note_type = node
        .children()
        .find(|child| child.has_tag_name("type"))
        .and_then(|child| child.text())?
        .trim()
        .to_ascii_lowercase();

    let ppq = ppq as Tick;
    let mut dur = match note_type.as_str() {
        "breve" => ppq.saturating_mul(8),
        "whole" => ppq.saturating_mul(4),
        "half" => ppq.saturating_mul(2),
        "quarter" => ppq,
        "eighth" => ppq / 2,
        "16th" => ppq / 4,
        "32nd" => ppq / 8,
        "64th" => ppq / 16,
        "128th" => ppq / 32,
        "256th" => ppq / 64,
        _ => return None,
    };

    if dur <= 0 {
        dur = 1;
    }

    let dots = node
        .children()
        .filter(|child| child.has_tag_name("dot"))
        .count();
    let mut add = dur / 2;
    for _ in 0..dots {
        if add <= 0 {
            break;
        }
        dur = dur.saturating_add(add);
        add /= 2;
    }

    if let Some(time_mod) = node
        .children()
        .find(|child| child.is_element() && child.has_tag_name("time-modification"))
    {
        let actual = time_mod
            .children()
            .find(|child| child.has_tag_name("actual-notes"))
            .and_then(|child| child.text())
            .and_then(|text| text.trim().parse::<Tick>().ok())
            .unwrap_or(0);
        let normal = time_mod
            .children()
            .find(|child| child.has_tag_name("normal-notes"))
            .and_then(|child| child.text())
            .and_then(|text| text.trim().parse::<Tick>().ok())
            .unwrap_or(0);

        if actual > 0 && normal > 0 {
            dur = (dur.saturating_mul(normal) + actual / 2) / actual;
            dur = dur.max(1);
        }
    }

    Some(dur.max(1))
}

fn parse_beats(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if !text.contains('+') {
        return text.parse::<i64>().ok();
    }
    let mut sum = 0i64;
    let mut any = false;
    for part in text.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Ok(value) = part.parse::<i64>() {
            sum = sum.saturating_add(value);
            any = true;
        }
    }
    any.then_some(sum)
}

fn measure_length_ticks(ppq: u16, beats: i64, beat_type: i64) -> Tick {
    if beats <= 0 || beat_type <= 0 {
        return 0;
    }
    let base = ppq as i64 * 4;
    base.saturating_mul(beats).div_euclid(beat_type)
}

fn parse_ties(node: &roxmltree::Node) -> (bool, bool) {
    let mut tie_start = false;
    let mut tie_stop = false;

    for child in node.children().filter(|n| n.is_element()) {
        if child.has_tag_name("tie") || child.has_tag_name("tied") {
            match child.attribute("type").unwrap_or("").trim() {
                "start" => tie_start = true,
                "stop" => tie_stop = true,
                _ => {}
            }
        }
        if child.has_tag_name("notations") {
            for tied in child
                .descendants()
                .filter(|n| n.is_element() && n.has_tag_name("tied"))
            {
                match tied.attribute("type").unwrap_or("").trim() {
                    "start" => tie_start = true,
                    "stop" => tie_stop = true,
                    _ => {}
                }
            }
        }
    }

    (tie_start, tie_stop)
}

fn parse_note(node: &roxmltree::Node) -> Option<u8> {
    let pitch = node.children().find(|child| child.has_tag_name("pitch"))?;
    let step = pitch
        .children()
        .find(|child| child.has_tag_name("step"))
        .and_then(|child| child.text())?;
    let octave = pitch
        .children()
        .find(|child| child.has_tag_name("octave"))
        .and_then(|child| child.text())
        .and_then(|text| text.parse::<i32>().ok())?;
    let alter = pitch
        .children()
        .find(|child| child.has_tag_name("alter"))
        .and_then(|child| child.text())
        .and_then(|text| text.parse::<i32>().ok())
        .unwrap_or(0);

    let base = match step {
        "C" => 0,
        "D" => 2,
        "E" => 4,
        "F" => 5,
        "G" => 7,
        "A" => 9,
        "B" => 11,
        _ => return None,
    };

    let midi_note = (octave + 1) * 12 + base + alter;
    if !(0..=127).contains(&midi_note) {
        return None;
    }
    Some(midi_note as u8)
}

fn parse_hand(node: &roxmltree::Node) -> Option<Hand> {
    let staff = node
        .children()
        .find(|child| child.has_tag_name("staff"))
        .and_then(|child| child.text())
        .and_then(|text| text.parse::<u8>().ok());
    match staff {
        Some(1) => Some(Hand::Right),
        Some(2) => Some(Hand::Left),
        _ => None,
    }
}

fn build_tempo_map(tempo_points: BTreeMap<Tick, u32>) -> Vec<TempoPoint> {
    let mut map: Vec<TempoPoint> = tempo_points
        .into_iter()
        .map(|(tick, us_per_quarter)| TempoPoint {
            tick,
            us_per_quarter,
        })
        .collect();

    if map.is_empty() || map[0].tick != 0 {
        map.insert(
            0,
            TempoPoint {
                tick: 0,
                us_per_quarter: 500_000,
            },
        );
    }

    map.sort_by_key(|point| point.tick);
    map
}

fn build_targets(note_events: &[NoteEvent]) -> Vec<TargetEvent> {
    let mut grouped: BTreeMap<Tick, TargetGroup> = BTreeMap::new();
    for event in note_events {
        let entry = grouped
            .entry(event.tick)
            .or_insert_with(|| (Vec::new(), event.measure_index));
        entry.0.push((event.note, event.hand));
    }

    let mut targets = Vec::new();
    let mut next_id = 1u64;
    for (tick, (notes, measure_index)) in grouped {
        let mut unique_notes: Vec<u8> = notes.iter().map(|(note, _)| *note).collect();
        unique_notes.sort_unstable();
        unique_notes.dedup();

        let hand = resolve_hand(&notes);
        targets.push(TargetEvent {
            id: next_id,
            tick,
            notes: unique_notes,
            hand,
            measure_index,
        });
        next_id += 1;
    }
    targets
}

fn apply_rearticulation_gaps(note_events: &mut [NoteEvent]) {
    let mut groups: HashMap<(u8, Option<Hand>), Vec<usize>> = HashMap::new();
    for (idx, event) in note_events.iter().enumerate() {
        groups
            .entry((event.note, event.hand))
            .or_default()
            .push(idx);
    }

    for indices in groups.values_mut() {
        indices.sort_by_key(|idx| note_events[*idx].tick);
        for pair in indices.windows(2) {
            let a = pair[0];
            let b = pair[1];
            let start = note_events[a].tick;
            let end = start + note_events[a].duration_ticks;
            let next_start = note_events[b].tick;
            if next_start <= start {
                continue;
            }
            if next_start <= end {
                let new_dur = (next_start - start - 1).max(1);
                note_events[a].duration_ticks = note_events[a].duration_ticks.min(new_dur);
            }
        }
    }
}

fn build_playback_events(
    note_events: &[NoteEvent],
    cc64_events: &[PlaybackMidiEvent],
) -> Vec<PlaybackMidiEvent> {
    let mut events = build_note_playback_events(note_events);
    events.extend(cc64_events.iter().cloned());
    events.sort_by(|a, b| {
        a.tick
            .cmp(&b.tick)
            .then_with(|| event_rank(&a.event).cmp(&event_rank(&b.event)))
            .then_with(|| event_note_key(&a.event).cmp(&event_note_key(&b.event)))
    });
    events
}

fn build_note_playback_events(note_events: &[NoteEvent]) -> Vec<PlaybackMidiEvent> {
    let mut events = Vec::new();
    for event in note_events {
        events.push(PlaybackMidiEvent {
            tick: event.tick,
            event: MidiLikeEvent::NoteOn {
                note: event.note,
                velocity: event.velocity.max(1),
            },
            hand: event.hand,
        });
        events.push(PlaybackMidiEvent {
            tick: event.tick + event.duration_ticks,
            event: MidiLikeEvent::NoteOff { note: event.note },
            hand: event.hand,
        });
    }
    events
}

fn event_rank(event: &MidiLikeEvent) -> u8 {
    match event {
        MidiLikeEvent::Cc64 { value } => {
            if *value >= 64 {
                0
            } else {
                3
            }
        }
        MidiLikeEvent::NoteOff { .. } => 1,
        MidiLikeEvent::NoteOn { .. } => 2,
    }
}

fn event_note_key(event: &MidiLikeEvent) -> u8 {
    match event {
        MidiLikeEvent::NoteOn { note, .. } => *note,
        MidiLikeEvent::NoteOff { note } => *note,
        MidiLikeEvent::Cc64 { .. } => 0,
    }
}

fn parse_dynamics_mark(direction_type: &roxmltree::Node) -> Option<u8> {
    let dynamics = direction_type
        .children()
        .find(|node| node.is_element() && node.has_tag_name("dynamics"))?;

    for child in dynamics.children().filter(|node| node.is_element()) {
        let name = child.tag_name().name();
        if name == "other-dynamics" {
            if let Some(text) = child.text() {
                if let Some(vel) = dynamics_tag_velocity(text.trim()) {
                    return Some(vel);
                }
            }
            continue;
        }
        if let Some(vel) = dynamics_tag_velocity(name) {
            return Some(vel);
        }
    }

    None
}

fn dynamics_tag_velocity(tag: &str) -> Option<u8> {
    let tag = tag.trim().trim_end_matches('.').to_ascii_lowercase();
    let vel = match tag.as_str() {
        "pppp" => 16,
        "ppp" => 24,
        "pp" => 34,
        "p" => 46,
        "mp" => 58,
        "mf" => 74,
        "f" => 92,
        "ff" => 108,
        "fff" => 120,
        "ffff" => 127,
        "sfz" | "sf" | "fz" => 112,
        _ => return None,
    };
    Some(vel)
}

fn parse_dynamics_words(direction_type: &roxmltree::Node) -> Option<u8> {
    for words in direction_type
        .children()
        .filter(|node| node.is_element() && node.has_tag_name("words"))
    {
        let Some(text) = words.text() else {
            continue;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(vel) = dynamics_tag_velocity(trimmed) {
            return Some(vel);
        }
    }
    None
}

fn parse_pedal_words(direction_type: &roxmltree::Node, pedal_down: bool) -> Option<bool> {
    for words in direction_type
        .children()
        .filter(|node| node.is_element() && node.has_tag_name("words"))
    {
        let Some(text) = words.text() else {
            continue;
        };
        let raw = text.trim();
        if raw.is_empty() {
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        if lower.contains("ped") {
            return Some(true);
        }
        if lower.contains('*') || lower.contains("release") {
            return Some(false);
        }
        if lower.contains("senza ped") {
            return Some(false);
        }
        if lower == "simile" && pedal_down {
            return Some(true);
        }
    }
    None
}

fn parse_velocity(value: &str) -> Option<u8> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(v) = value.parse::<f32>() {
        if v <= 1.0 {
            return Some((v * 127.0).round().clamp(0.0, 127.0) as u8);
        }
        if v <= 100.0 {
            return Some((v / 100.0 * 127.0).round().clamp(0.0, 127.0) as u8);
        }
        return Some(v.round().clamp(0.0, 127.0) as u8);
    }
    None
}

fn parse_pedal_mark(pedal_node: &roxmltree::Node, pedal_down: bool) -> Option<bool> {
    let kind = pedal_node.attribute("type").unwrap_or("").trim();
    match kind {
        "start" => Some(true),
        "stop" => Some(false),
        "change" => Some(!pedal_down),
        _ => None,
    }
}

fn parse_pedal_value(value: &str) -> Option<bool> {
    let value = value.trim();
    if let Ok(v) = value.parse::<f32>() {
        return Some(v >= 64.0);
    }

    match value.to_ascii_lowercase().as_str() {
        "yes" | "true" | "on" => Some(true),
        "no" | "false" | "off" => Some(false),
        _ => None,
    }
}

fn emit_cc64_change(
    out: &mut Vec<PlaybackMidiEvent>,
    tick: Tick,
    pedal_down: &mut bool,
    down: bool,
) {
    if *pedal_down == down {
        return;
    }
    *pedal_down = down;
    out.push(PlaybackMidiEvent {
        tick,
        event: MidiLikeEvent::Cc64 {
            value: if down { 127 } else { 0 },
        },
        hand: None,
    });
}

fn resolve_hand(notes: &[(u8, Option<Hand>)]) -> Option<Hand> {
    let mut current = None;
    for (_, hand) in notes {
        if let Some(hand) = hand {
            if let Some(existing) = current {
                if existing != *hand {
                    return None;
                }
            } else {
                current = Some(*hand);
            }
        }
    }
    current
}

fn read_musicxml_file(path: &Path) -> Result<String, MusicXmlImportError> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("mxl") {
        return read_mxl_archive(path);
    }
    std::fs::read_to_string(path).map_err(|e| MusicXmlImportError::Io(e.to_string()))
}

fn read_mxl_archive(path: &Path) -> Result<String, MusicXmlImportError> {
    let data = std::fs::read(path).map_err(|e| MusicXmlImportError::Io(e.to_string()))?;
    let mut archive = ZipArchive::new(std::io::Cursor::new(data))
        .map_err(|e| MusicXmlImportError::Parse(e.to_string()))?;

    let container_xml = if let Ok(mut container) = archive.by_name("META-INF/container.xml") {
        let mut xml = String::new();
        container
            .read_to_string(&mut xml)
            .map_err(|e| MusicXmlImportError::Io(e.to_string()))?;
        Some(xml)
    } else {
        None
    };

    if let Some(container_xml) = container_xml {
        if let Ok(doc) = Document::parse(&container_xml) {
            if let Some(full_path) = doc
                .descendants()
                .find(|node| node.has_tag_name("rootfile"))
                .and_then(|node| node.attribute("full-path"))
            {
                if let Ok(mut rootfile) = archive.by_name(full_path) {
                    let mut xml = String::new();
                    rootfile
                        .read_to_string(&mut xml)
                        .map_err(|e| MusicXmlImportError::Io(e.to_string()))?;
                    return Ok(xml);
                }
            }
        }
    }

    for idx in 0..archive.len() {
        let mut file = archive
            .by_index(idx)
            .map_err(|e| MusicXmlImportError::Parse(e.to_string()))?;
        let name = file.name().to_string();
        if name.ends_with(".xml") && !name.starts_with("META-INF/") {
            let mut xml = String::new();
            file.read_to_string(&mut xml)
                .map_err(|e| MusicXmlImportError::Io(e.to_string()))?;
            return Ok(xml);
        }
    }

    Err(MusicXmlImportError::Unsupported(
        "mxl archive missing MusicXML payload".to_string(),
    ))
}
