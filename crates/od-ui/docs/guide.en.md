# Contour guide

Contour turns a description into a design: pages, prototypes, slide decks, documents, images and video. Everything runs on this computer. A coding agent you already use (Claude Code, Codex, Gemini CLI…) or your own API key does the work; nothing is uploaded unless you configure a provider.

## Getting started

1. **Pick an agent.** On first run, choose an installed agent and let Contour test it, or paste an API key.
2. **Bring your brand (optional).** Give your website and Contour extracts a design system (palette, type, components). New projects start with it.
3. **Describe what you want** on Home and press Create. The Studio opens and the agent starts.

## Home

- The output chips (Prototype, Slide deck, Document, Image, Video…) set what the agent makes. Media chips show ratio, duration and resolution.
- **+** attaches files, a plugin, a skill, an MCP server or assets from your media library. Type **@** to mention the same things inline, **/** for commands.
- **Recommended for you** suggests a starter based on what you made before. The tabs below open your designs, templates, design systems and image or video prompt templates.

## Studio

- Chat on the left, preview in the middle, files on the right.
- The preview toolbar has Comment, Inspect, Edit text, Draw, Tweaks, Palette and Lint. Comments you add are attached to the next message.
- **Export** saves HTML, ZIP, PDF, images, PowerPoint, MP4, Markdown, or the source of a React component (`.jsx`/`.tsx`).
- **Design Jury** scores a design over several rounds before it ships. Press **Interrupt** (or Esc) to stop it; the best round so far ships. **Replay** plays a finished jury back.
- When the agent generates images, audio or video, a row in the chat shows each one as it is made.

## Design systems

Create one from a website, a GitHub repository, files, a pasted DESIGN.md or CSS variables, or copy an existing one. Upload font and logo files; until brand fonts are added, previews use substitutes.

## Memory

Settings ▸ Memory holds facts every project should respect. Turn memory off with the main switch. The memory hooks choose what runs on their own: your profile, rewriting short requests, self-checking against your rules and learning from chats. **Import** reads facts from a Markdown file; **Edit MEMORY.md** chooses which facts reach the prompt. In a chat, start a message with "Remember that…" to save a fact for that project.

## DeepSeek Harness {#deepseek-harness}

Contour runs your own `dsh` CLI through its `open-design` profile. It does not install either one.

1. Install `dsh`. The bootstrap installer also installs a compatible Node.js and pnpm, then opens the Harness web UI, where you save your DeepSeek API key:
   - macOS / Linux: `curl -fsSL 'https://open-design.ai/install-dsh.sh?version=1' | sh`
   - Windows PowerShell: `& ([scriptblock]::Create((irm 'https://open-design.ai/install-dsh.ps1?version=1')))`
2. Add the `open-design` profile. Build its package from the upstream source (`nexu-io/open-design`, `packages/dsh-runtime`), then add it with `dsh`:
   - `pnpm --filter @open-design/dsh-runtime build`
   - `pnpm -C packages/dsh-runtime pack --pack-destination <folder>`
   - `dsh plugin --profile open-design add <folder>/open-design-dsh-runtime-0.1.0.tgz`
   - `dsh --profile open-design --probe` prints one line of JSON when the profile works.
3. Back in Contour, open the DeepSeek Harness card and choose **Detect again**. The agent is selected as soon as it passes the check.

The profile lives in `~/.dsh/profiles/open-design`, or under `$DSH_HOME` when that is set.

## Keyboard

- `Cmd/Ctrl+K` command palette, `?` all shortcuts, `Cmd/Ctrl+,` Settings.
- In the Studio: `Cmd/Ctrl+B` files, `Cmd/Ctrl+T` new tab, `Cmd/Ctrl+P` find a file.

## When something goes wrong

- **The agent does not answer:** Settings ▸ Models ▸ Test, or Refresh on onboarding.
- **Export to PDF or images fails:** install Chrome, Edge or Chromium, or set `OD_CHROME_PATH`.
- **Report an issue:** Help ▸ Report Issue copies a diagnostics summary (version, platform, agents, recent runs, nothing secret) to the clipboard and opens the run log folder. Paste it wherever you report the problem. Settings ▸ About ▸ Export diagnostics saves a full archive.
