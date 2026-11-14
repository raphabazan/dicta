## ADDED Requirements

### Requirement: Recover a valid recording after an empty segment response
The application SHALL distinguish digital silence from a provider returning an explicitly empty text field on non-silent input.

#### Scenario: Empty final response with earlier successful segments
- **WHEN** a segment with appreciable signal receives an empty transcription
- **THEN** retry only that segment once without the hint, and deliver the assembled text through the normal finalization path if it succeeds.

#### Scenario: An unresolved segment remains
- **WHEN** a second attempt does not yield text
- **THEN** preserve the recording for recovery and allow the available preview to be copied without marking the session complete.

#### Scenario: Empty response on a final segment at the microphone noise floor
- **WHEN** the provider returns a valid empty text for a stop or recovery tail whose new PCM16 samples have peak amplitude at most 16 and RMS at most 2
- **THEN** complete the tail without adding words or blocking earlier segments. Transport errors and malformed responses remain recoverable.

#### Scenario: Silence after an already transcribed segment
- **WHEN** new samples after the overlap contain only digital silence within one PCM16 step
- **THEN** omit those samples from ASR, retain earlier text and leave the previous paste result untouched if the entire recording is silent.
