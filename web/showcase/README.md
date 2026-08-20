# Akra Hookers showcase recorder

This harness records the real React product UI against an isolated, deterministic API fixture. It never opens a user's database, mutates Codex hooks, sends captured prompts, or invokes an external LLM.

Run from the repository root:

```powershell
npm --prefix web run showcase:record
```

This always creates the silent master used by the product and release-page
workflow. To create the separate personal-creator YouTube edition, download
`Cartoon, Jéja - On & On (feat. Daniel Levi) [NCS Release]` from the
[official NCS track page](https://ncs.io/onandon), then run:

```powershell
$env:SHOWCASE_YOUTUBE_MUSIC = "C:\path\to\official-ncs-download.mp3"
npm --prefix web run showcase:youtube
```

To record a fresh silent master and then mix the YouTube edition in one run:

```powershell
$env:SHOWCASE_YOUTUBE_MUSIC = "C:\path\to\official-ncs-download.mp3"
npm --prefix web run showcase:record:youtube
```

The recorder demonstrates capture health, date/project/activity filters, request and result evidence, asynchronous result-summary regeneration, log curation, human review of an AI grouping proposal, work promotion, and explicit work-edge creation/removal.

Generated files are intentionally ignored by Git:

- `artifacts/Akra-Hookers-Showcase-QHD.webm`
- `artifacts/Akra-Hookers-Showcase-QHD.mp4`
- `artifacts/Akra-Hookers-Showcase-QHD-YouTube-NCS.mp4`

The first two files remain silent. The `YouTube-NCS` file is a separate mix at
-18 LUFS with short entry and exit fades; its video stream is copied from the
silent QHD master without another video encode. Copy the complete attribution
from `youtube-description.txt` into the YouTube description.

The official MP3 used for the 2026-08-20 render had SHA-256
`8dadfc64562b8234f5bf3b3c2ea536cbe4bc088a8df9bd923b0219035fcf0eae`.
This fingerprint documents the production input without redistributing it;
NCS may replace its downloadable encoding in the future.

Do not commit or redistribute the downloaded NCS audio file. Do not publish the
NCS edition on the Akra release page, in paid ads, or as brand/service product
promotion without the commercial permission required by the current
[NCS usage policy](https://ncs.io/usage-policy). The silent master is the only
release-page artifact.

The recorder uses a native 2560×1440 browser viewport and renders the UI at 1.6× browser zoom. This preserves the 1600×900 composition while drawing text, borders, and canvas lines directly into the full QHD frame instead of enlarging a lower-resolution export.

The cursor, chapter cards, and title cards exist only in the recording test. They are not bundled into the application.
