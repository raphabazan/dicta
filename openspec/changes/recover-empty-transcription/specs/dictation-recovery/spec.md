## ADDED Requirements

### Requirement: Recover a valid recording after an empty segment response
The application SHALL distinguish digital silence from a provider returning an explicitly empty text field on non-silent input.

#### Scenario: Empty final response with earlier successful segments
- **WHEN** a segment with appreciable signal receives an empty transcription
- **THEN** retry only that segment once without the hint, and deliver the assembled text through the normal finalization path if it succeeds.

#### Scenario: An unresolved segment remains
- **WHEN** a second attempt does not yield text
- **THEN** preserve the recording for recovery. At the end of a transcription-only dictation, save the nonempty contiguous preview as partial history and attempt one paste using the original destination. Keep it available to the last-text shortcut. On successful recovery, promote the same history entry to the complete text without automatically pasting again. Interrupted sessions and command mode must not paste a partial result.

#### Scenario: Empty response on a final segment at the microphone noise floor
- **WHEN** the provider returns a valid empty text for a stop or recovery tail whose new PCM16 samples have peak amplitude at most 16 and RMS at most 2
- **THEN** complete the tail without adding words or blocking earlier segments. Transport errors and malformed responses remain recoverable.

#### Scenario: Silence after an already transcribed segment
- **WHEN** new samples after the overlap contain only digital silence within one PCM16 step
- **THEN** omit those samples from ASR, retain earlier text and leave the previous paste result untouched if the entire recording is silent.
