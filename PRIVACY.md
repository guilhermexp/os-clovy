# Clovy privacy

Recordings, transcripts, notes, and session history are stored on-device.
Audio or text leaves the device when a requested operation uses a provider:
Groq for transcription, OpenRouter for remote notes and optional dictation
cleanup, and fal for images/video. These calls use your own key directly.
With Local (Parakeet Ultra) selected, dictation, note transcription, turn
packs, retries, and live preview process audio on-device, without transcription
requests or fallback to Groq. Installing the optional model downloads pinned
files from Hugging Face; those downloads send no audio or provider key.
Optional text cleanup and note generation still use their separately selected
provider or CLI and may send the resulting text off-device.
CLI chat and CLI note generation use the CLI's account; configured endpoints
receive the content sent to them. Provider retention depends on that route.

Clovy sends no usage telemetry or issue reports, requires no Clovy account,
and offers no sharing service. There is no Clovy inference server or local
bearer token. Keys are stored in the operating system credential store and
sent only to their own provider. Media downloads carry no provider credentials.
Clovy's local MCP tools retain their turn-bound approval and data boundaries.
