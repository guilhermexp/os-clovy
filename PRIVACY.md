# Clovy privacy

Clovy is private by architecture. Recordings, transcripts, notes, and agent
session content stay on-device except when the user asks Clovy to run model
inference through Clovy API or a configured own provider.

## No telemetry

Clovy sends no usage statistics, telemetry, or issue reports, and has no
account sign-in. It talks only to the Clovy API server you configure
(`http://127.0.0.1:8080` by default, `CLOVY_API_URL` overrides) and to the
model providers you set up yourself. With your own Groq or OpenRouter key
saved, dictation and notes call that provider directly with only that key.

## Meeting video

On macOS, you can opt in to recording the detected meeting window. This is
off by default. The video helper captures only that window, without audio,
and requires its own Screen Recording permission. Microphone and system
audio continue through the separate audio pipeline. Pausing the recording
also pauses the video.
The desktop's separate Screen Recording grant is needed to enumerate meeting
windows. Without access to those window details, the video option is hidden.

Video is stored locally beside the session's audio as `window.partial.mp4`
during capture and `window.mp4` after finalization. Deleting the recording or
its note deletes the video. Video can use substantially more disk space than
audio. A crash may leave the last incomplete fragment unrecoverable.

When Notes uses the Claude CLI, Clovy can run your installed `video-analysis`
skill with local Python and FFmpeg. Extracted frames and transcript content
are then processed by Claude through your configured Claude service. They
are not sent to Clovy API for video analysis. The per-call temporary evidence
directory is deleted after analysis, and Claude session persistence is
disabled. Clovy does not bundle the skill, Python or FFmpeg.

Other Notes engines and unsuccessful analysis use only the transcript, with
a notice explaining that the video was not analyzed. Turning video on does
not enable full-display capture or tools in other one-shot generation calls.
