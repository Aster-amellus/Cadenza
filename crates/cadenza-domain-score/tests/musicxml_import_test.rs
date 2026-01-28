use cadenza_domain_score::import_musicxml_str;
use cadenza_ports::midi::MidiLikeEvent;

fn note_on_ticks(score: &cadenza_domain_score::Score) -> Vec<(i64, u8)> {
    let track = score.tracks.first().expect("track");
    track
        .playback_events
        .iter()
        .filter_map(|e| match e.event {
            MidiLikeEvent::NoteOn { note, .. } => Some((e.tick, note)),
            _ => None,
        })
        .collect()
}

fn note_off_ticks(score: &cadenza_domain_score::Score) -> Vec<(i64, u8)> {
    let track = score.tracks.first().expect("track");
    track
        .playback_events
        .iter()
        .filter_map(|e| match e.event {
            MidiLikeEvent::NoteOff { note } => Some((e.tick, note)),
            _ => None,
        })
        .collect()
}

#[test]
fn musicxml_chord_notes_share_start_tick() {
    let xml = r#"
<score-partwise version="3.1">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>1</duration>
        <staff>1</staff>
      </note>
      <note>
        <chord/>
        <pitch><step>E</step><octave>4</octave></pitch>
        <duration>1</duration>
        <staff>1</staff>
      </note>
    </measure>
  </part>
</score-partwise>
"#;

    let score = import_musicxml_str(xml).expect("import ok");
    let track = score.tracks.first().expect("track");
    assert_eq!(track.targets.len(), 1);
    assert_eq!(track.targets[0].tick, 0);
    assert_eq!(track.targets[0].notes, vec![60, 64]);

    let mut ons = note_on_ticks(&score);
    ons.sort();
    assert_eq!(ons, vec![(0, 60), (0, 64)]);

    let mut offs = note_off_ticks(&score);
    offs.sort();
    assert_eq!(offs, vec![(480, 60), (480, 64)]);
}

#[test]
fn musicxml_backup_keeps_voices_aligned() {
    let xml = r#"
<score-partwise version="3.1">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>2</duration>
        <voice>1</voice>
        <staff>1</staff>
      </note>
      <backup><duration>2</duration></backup>
      <note>
        <pitch><step>E</step><octave>4</octave></pitch>
        <duration>2</duration>
        <voice>2</voice>
        <staff>1</staff>
      </note>
    </measure>
  </part>
</score-partwise>
"#;

    let score = import_musicxml_str(xml).expect("import ok");
    let track = score.tracks.first().expect("track");
    assert_eq!(track.targets.len(), 1);
    assert_eq!(track.targets[0].tick, 0);
    assert_eq!(track.targets[0].notes, vec![60, 64]);
}

#[test]
fn musicxml_ties_merge_into_single_note() {
    let xml = r#"
<score-partwise version="3.1">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note><rest/><duration>3</duration></note>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>1</duration>
        <tie type="start"/>
        <staff>1</staff>
      </note>
    </measure>
    <measure number="2">
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>1</duration>
        <tie type="stop"/>
        <staff>1</staff>
      </note>
    </measure>
  </part>
</score-partwise>
"#;

    let score = import_musicxml_str(xml).expect("import ok");
    let track = score.tracks.first().expect("track");

    assert_eq!(track.targets.len(), 1);
    assert_eq!(track.targets[0].tick, 1440);
    assert_eq!(track.targets[0].notes, vec![60]);

    let ons = note_on_ticks(&score);
    assert!(ons.iter().any(|(t, n)| *t == 1440 && *n == 60));
    assert!(!ons.iter().any(|(t, n)| *t == 1920 && *n == 60));

    let offs = note_off_ticks(&score);
    assert!(offs.iter().any(|(t, n)| *t == 2400 && *n == 60));
}

#[test]
fn musicxml_clamps_notes_to_measure_end_without_tie() {
    let xml = r#"
<score-partwise version="3.1">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note><rest/><duration>3</duration></note>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>4</duration>
        <staff>1</staff>
      </note>
    </measure>
  </part>
</score-partwise>
"#;

    let score = import_musicxml_str(xml).expect("import ok");
    let offs = note_off_ticks(&score);
    assert!(offs.iter().any(|(t, n)| *t == 1920 && *n == 60));
}

#[test]
fn musicxml_pickup_measure_does_not_pad_to_time_signature() {
    let xml = r#"
<score-partwise version="3.1">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1" implicit="yes">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>1</duration>
        <staff>1</staff>
      </note>
    </measure>
    <measure number="2">
      <note>
        <pitch><step>D</step><octave>4</octave></pitch>
        <duration>1</duration>
        <staff>1</staff>
      </note>
    </measure>
  </part>
</score-partwise>
"#;

    let score = import_musicxml_str(xml).expect("import ok");
    let mut ons = note_on_ticks(&score);
    ons.sort();
    assert!(ons.contains(&(0, 60)));
    assert!(ons.contains(&(480, 62)));
}

#[test]
fn musicxml_infers_duration_from_type_when_missing() {
    let xml = r#"
<score-partwise version="3.1">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <type>quarter</type>
        <staff>1</staff>
      </note>
      <note>
        <pitch><step>D</step><octave>4</octave></pitch>
        <type>quarter</type>
        <staff>1</staff>
      </note>
    </measure>
  </part>
</score-partwise>
"#;

    let score = import_musicxml_str(xml).expect("import ok");
    let mut ons = note_on_ticks(&score);
    ons.sort();
    assert_eq!(ons, vec![(0, 60), (480, 62)]);

    let mut offs = note_off_ticks(&score);
    offs.sort();
    assert_eq!(offs, vec![(480, 60), (960, 62)]);
}
