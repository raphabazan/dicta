# Recover empty transcription responses

## Why
The last segment of a recording can receive HTTP 200 with an empty transcript. This currently prevents saving and delivering earlier successful segments.

## What Changes
- Skip PCM16 digital silence in new samples, excluding overlap.
- For an explicitly empty transcript on non-silent audio, try the same segment once more without prompt context, using GPT-Transcribe when the selected model was Whisper.
- Preserve unresolved audio for recovery and expose a labelled copy action for the available preview.

## Impact
Only a failed segment may incur an additional transcription attempt. Existing history and the one-paste delivery path remain in use. A non-empty transcription is not proof of accuracy.
