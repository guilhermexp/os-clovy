# Clovy privacy

Clovy is private by architecture. Recordings, transcripts, notes, and agent
session content stay on-device except when the user asks Clovy to run model
inference through Clovy API.

## No telemetry

Clovy sends no usage statistics, telemetry, or issue reports, and has no
account sign-in. It talks only to the Clovy API server you configure
(`http://127.0.0.1:8080` by default, `CLOVY_API_URL` overrides) and to the
model providers you set up yourself.
