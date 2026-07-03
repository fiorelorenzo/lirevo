# Using Lirevo

Lirevo is a **menu-bar app**. A Dock icon appears while a window is open;
closing the home or settings window hides it back to the tray (the Dock icon
disappears) instead of quitting. Reopen it from the tray's **Show Lirevo** item.

## First run: the setup wizard

On first launch a short wizard gets you ready to dictate:

1. **Pick your dictation language.** The bundled speech model (Parakeet TDT v3)
   covers 25 European languages; auto-detect is the default.
2. **Download your models.** Two progress cards — the dictation (STT) model and
   the cleanup (LLM) model — with retry on error. You continue once both finish.
3. **Grant permissions.** Microphone (to capture audio) and Accessibility (for
   the global hotkey and to type into other apps). Accessibility must be toggled
   on manually in System Settings; macOS has no programmatic grant.
4. **Finish setup.** Choose your push-to-talk hotkey (default: Right Option), and
   toggle **Launch at login** and **Smart Microphone**.

## Push-to-talk dictation

Hold the hotkey anywhere on the system and speak. Release to transcribe, clean
up, and inject into the focused app. The pipeline runs in three stages:

1. **Speech-to-text** transcribes your audio, streaming a live partial transcript
   into the overlay while you talk.
2. **Cleanup** runs a small local LLM that removes disfluencies and adds
   punctuation **without translating**. If no cleanup model is configured, the
   raw transcript is typed as-is; if cleanup fails, Lirevo falls back to the raw
   transcript.
3. **Injection** types the text at your cursor via the macOS Accessibility API,
   with a clipboard-paste fallback for apps that do not expose a standard text
   element.

A transparent, notch-safe **overlay** appears centred on screen the moment you
start recording — a live waveform plus the streaming transcript while you speak,
then a processing animation that persists through transcription and cleanup
until the final text is injected.

## The menu-bar tray

The tray icon is a monochrome waveform whose amplitude encodes the active energy
profile (low = Power Saver, medium = Balanced, tall = Performance). It also
reflects state: an animated pulse while models load, a recording indicator while
you dictate, an error icon if models fail, and an attention badge when a required
permission is missing. The tray menu has a status line, the hotkey hint, an
**Energy Profile** submenu, **Show Lirevo**, **Settings…**, **Check for
updates**, and **Quit**.

## Energy profiles

Lirevo is resource-aware. An **Energy Profile** controls how long models stay
resident in memory, how many CPU threads the LLM uses, and when models are
unloaded on battery. Set it from the tray's Energy Profile submenu or in
**Settings → General → App**:

| Profile | LLM idle-unload | STT idle-unload | Models kept warm |
| --- | --- | --- | --- |
| Power Saver | 10 s | 60 s | no |
| Balanced (default) | 2 min | 5 min | yes |
| Performance | 10 min | 15 min | yes |

In **Auto** mode (the default), Lirevo watches battery level, AC state, thermal
and memory pressure, and foreground-app CPU, and picks the profile for you,
switching to Power Saver under pressure (a toast explains why). Models that get
idle-unloaded reload transparently on your next dictation, so they always appear
"ready".

## Smart Microphone

When your primary mic is a Bluetooth device (AirPods, say), opening it for
capture forces the Bluetooth link out of stereo (A2DP) into mono handsfree
(HFP), killing stereo playback for the duration. **Smart Microphone** (on by
default; **Settings → General → Dictation**) avoids this: if a Bluetooth output
is actively playing and your mic is also Bluetooth, dictation routes to a backup
mic (built-in by default, configurable) so your audio keeps playing in stereo.

## Dictation history

If **Record dictation history** is enabled (**Settings → General → App**), every
dictation is saved to a local SQLite database on your device and shown on the
home screen. Each entry expands to show the raw transcript, the cleaned text,
which models ran, the target app, the input device used, timings, and language.
History never leaves your machine; clear it any time from the home screen.

## Permissions

Lirevo needs two macOS permissions:

- **Microphone** — to capture your speech.
- **Accessibility** — to register the global hotkey and to type into other apps.

If either is missing, the home screen shows a warning banner and the tray icon
shows an attention badge, with buttons to grant or open the relevant System
Settings pane.

## Text injection: known limitations

- The **Accessibility (AXUIElement) path** is preferred and works in most native
  Cocoa apps (Safari, Notes, TextEdit, and similar). It inserts at the cursor or
  replaces the current selection.
- The **pasteboard fallback** kicks in automatically when the Accessibility path
  cannot reach an app's text element — typical for Electron apps (VS Code,
  Cursor, Slack, Discord) — or when you enable **Always use pasteboard** in
  Settings.
- During the pasteboard fallback the clipboard is temporarily overwritten with
  your text and then restored. **Non-string clipboard content (images, files) is
  not preserved** and is lost. Disable the fallback with **Always use
  pasteboard** only if you accept clipboard-paste for every injection.
- If injection lands before the target app is ready, raise the **Paste delay**
  slider (**Settings → General → Text injection**, default 120 ms).

## Models

The shipped app downloads everything for you from the setup wizard — there are
no files to place manually.

### Speech-to-text (STT)

A single model ships today (authoritative catalog:
`app/src-tauri/src/stt/catalog.rs`):

| Model | Size | Languages | License |
| --- | --- | --- | --- |
| **Parakeet TDT v3** (default) | ~644 MB | 25 European languages | CC-BY-4.0 |

The GGUF weights (`tdt-0.6b-v3-q4_k.gguf` from `mudler/parakeet-cpp-gguf`) are
downloaded into the app data directory's `models/` folder on first use. There
are no separate CoreML encoders to fetch — on Apple Silicon `parakeet-cpp`
accelerates via the dynamic ggml Metal backend.

### Language model (LLM cleanup)

LLM cleanup models are GGUF files downloaded in-app from an embedded catalog
(`crates/inference-core/data/model_catalog.json`):

| Model | Filename | Size | Recommended |
| --- | --- | --- | --- |
| Qwen3 4B | `Qwen3-4B-Instruct-2507-Q4_K_M.gguf` | ~2.5 GB | no |
| Llama 3.2 3B | `Llama-3.2-3B-Instruct-Q4_K_M.gguf` | ~2.0 GB | no |
| Qwen3 1.7B | `Qwen3-1.7B-Q4_K_M.gguf` | ~1.1 GB | no |
| **Gemma 3 1B** | `gemma-3-1b-it-Q4_K_M.gguf` | ~800 MB | **yes** |
| Gemma 3 270M | `gemma-3-270m-it-Q4_K_M.gguf` | ~250 MB | no |

The wizard downloads the recommended model (currently **Gemma 3 1B**) by
default. LLM files are stored under the app data directory's `models/` folder
(for example `~/Library/Application Support/Lirevo/models/`). You can also point
the **Models** tab at an existing `.gguf` file with the file picker. The LLM
context size is configurable in **Settings → Models → Advanced** (default 4096
tokens).

## Where your data lives

Debug builds use a distinct bundle id (`ai.lirevo.app.dev`) and app-name-suffixed
directories so they never touch the release app's models, history, settings, or
macOS system state:

| Build type | Data directory | Log directory |
| --- | --- | --- |
| Release (`just dmg`) | `~/Library/Application Support/Lirevo` | `~/Library/Logs/Lirevo` |
| Debug (`just dev`, `just dev-bundle`) | `~/Library/Application Support/Lirevo (Dev)` | `~/Library/Logs/Lirevo (Dev)` |
