# Crypt — Project space (3D background) · design spec

This is the design reference for the 3D project view. The 3D world fills the background, and the chat sits in a semi-transparent panel on top. All values below come from the working prototype `crypt-lab.html`. To see the screen as it would ship, open that file and click **⤢ Vue app**.

---

## 1. Principles

1. **The background is the project.** Every visible object maps to something real: a project, a folder, a file, an agent. Nothing is decoration, except dust, pillars and fog, which stay very faint.
2. **Orange means "happening now" or "changed".** It is the only accent color. Everything else is white on black.
3. **The camera tells the story.** It follows the agent at work, pulls back to show the whole project, and drops on `/rewind`.
4. **The UI floats above the world.** The chat, metrics and title are flat layers over the 3D, and they drift slightly with the camera (parallax).

## 2. Tokens

| Token | Value | Use |
|---|---|---|
| `bg` | `#050505` | Background, fog color, ghost eyes |
| `fg` | `#F5F5F3` | Text, ghosts, slab edges, user prompt bubble |
| `accent` | `#FF5B1F` | Active or changed things, the RUN button, Kiro's horns, alerts |
| `fg/55` | `rgba(245,245,243,.55)` | Secondary text |
| `fg/32` | `rgba(245,245,243,.32)` | Placeholders, pending items |
| `line` | `rgba(245,245,243,.13)` | Panel borders |
| Font UI | Geist 400–700 | Titles, file and step names |
| Font mono | Geist Mono 400–600 | Chat text, labels, numbers (tabular figures) |

**Type scale** at the 1440 × 900 reference:

| Element | Size and weight |
|---|---|
| Step title | 64 px, 600, letter-spacing −0.04 em |
| Metric value | 28 px, 600 |
| Step name in the trace | 14 px, 600 |
| Chat text | 12.5–13.5 px mono |
| Labels | 10 px mono, uppercase, +0.16 em letter-spacing |

## 3. Screen layout

The reference frame is 1440 × 900. It scales uniformly to the window.

| Layer (bottom → top) | Position | Notes |
|---|---|---|
| 3D canvas | Full screen | WebGL, bloom post-process |
| 3D labels | Full screen | HTML positioned from 3D projection, no pointer events |
| Vignette | Full screen | Radial gradient that darkens the edges to 80 % |
| Header | x 48, y 34 | Ghost logo, CRYPT, tagline. Right side: live · project · model · F7 hub |
| **Chat panel** | x 48, y 92, 520 × 762 | See §4 |
| Step title | x 620, y 104 | Small index line (e.g. "04 / 06 · now") above the step name |
| Caption | x 620, y 806, width 640 | See §5 |
| Metrics | x 1288, y 104, width 104 | 6 rows, 94 px each, 1 px left rule, 2 px orange progress bar |
| Status bar | y 868, full width | connected · tok/s · p50 · context %. Nothing on the right |

## 4. Chat panel (semi-transparent)

- **Surface:**
  - Gradient from `fg` at 3 % alpha (top) to 0.75 % (bottom).
  - `backdrop-filter: blur(6px) saturate(130%)`.
  - Border 1 px `line`, radius 18.
  - `inset 0 1px 0 rgba(245,245,243,.16)` and a soft drop shadow.
- **Corner ticks:** 9 px orange L-shapes on the top-left and bottom-right corners, 1.5 px stroke.
- **Header (44 px):** three dots (the first one orange), the breadcrumb `crypt / chat-1 / model` with orange slashes, and a ⌘K key.
- **Message list:** anchored to the bottom, so new content pushes older content up. The top 36 px fade out with a mask. New blocks enter with an 8 px slide and fade over 0.45 s.
  - **User prompt:** white bubble with black mono text. Radius 16 / 16 / 4 / 16, aligned right.
  - **Trace:** 6 rows (Ask, Context, Think, Act, Critic, Answer), 29 px each. Each row has an isometric mini-slab icon, its index, name, detail and time. Sub-agents never appear here.

    | Row state | Look |
    |---|---|
    | Pending | 30 % white |
    | Active | Orange 10 % row background, orange icon and index, pulsing time |
    | Done | White text, icon filled with 26 % orange |

  - **Thinking line:** three pulsing orange dots and the current step in lower case ("act…").
  - **Answer:** 26 px ghost avatar and a glass bubble. The text reveals left to right over 1.6 s. Below it, a meta line in 10.5 px (`file +4 −1 · checkpoint N saved`).
  - **Kiro remark:** outline ghost with orange horns and an italic bubble, indented 38 px.
  - **System lines:** mono 11.5 px with an orange `›` (`/rewind`, `F7 hub`). Alerts are fully orange and pulse.
- **Input (46 px, bottom):** placeholder "Ask the crypt…", the `/agent` and `/tell` keys, and an orange **RUN** button. The button reads **RUNNING** while a request runs, and has a 30 px orange glow. While the user types, a blinking orange caret shows.

## 5. Caption (what is happening)

A single line under the 3D area, always in sync with the scene: `[WHO] what happens`.

| WHO chip | Style |
|---|---|
| `you` | White chip, black text |
| `main ghost` | White outline |
| `sub-agents`, `kiro`, `alert` | Orange outline, orange text |

Examples: "Reads src/hub/server.rs", "Patches src/hub/encode.rs +4 −1", "#1 is done → pushed off".

## 6. The 3D world

### 6.1 Mapping

| Real thing | 3D object | Size (world units) |
|---|---|---|
| Project (repo) | A district of folder plates. The active one has an orange dashed outline and the label "you are here". | Districts 3 500–9 000 apart |
| Folder | Rectangle outline on the floor, `fg` at 16 %. Path written on the floor in 9.5 px uppercase mono. | Files in a row, 150 apart, 85 padding |
| File | Stack of slabs. **1 slab ≈ 150 lines.** Name label above the top slab. | Slab 110 × 12 × 110, pitch 22 |
| Main agent | White 3D ghost, hovering 46 above the file it works on | Width 60 |
| Sub-agent | Smaller white ghost | Width 40 |
| Kiro | Ghost with two orange cones as horns, on its own perch slab (90 wide) next to the file it reviews | Width 36 |
| Web search | A floating slab the sub-agent builds outside the project | 130 wide |

### 6.2 Slab look

- **Box:** `fg` at 4 %, no depth write.
- **Edges:** 1 px. The opacity formula is `.3 + .55·read + .7·active + .25·changed`. The color blends from white to orange by `active + .6·changed`.
- **Top face:** opacity `.025 + .16·read + .24·active + .08·changed`, orange as soon as the slab is active or changed.
- **Under the file:** a radial glow on the floor, orange for active or changed (`.45·active + .3·changed`) and white for read (`.28`).

### 6.3 Ghost model

- **Shape:** extruded from the 2D ghost path (`M50 6C27 6 12 23 12 46V104L25 95 38 104 50 95 62 104 75 95 88 104V46C88 23 73 6 50 6Z`).
  - Depth 18, bevel 9 (thickness) and 7 (size), 5 segments.
- **Material:** standard material, color `#DEDED9`, roughness 0.55, emissive white at about 0.06. It is matte on purpose, so it does not over-glow.
- **Eyes:** black, slightly flattened ellipsoids sitting on the front face.
- **Motion:**
  - Always faces the camera (yaw only), plus a slow sway of ±0.45 rad.
  - Bobs 5 units at 2.6 rad/s.
  - A soft white contact glow sits on the surface below it.
- **Lights:** a hemisphere light at 0.62, one directional light at 0.7, and an orange point light above the file being edited.

### 6.4 Environment

- **Floor:**
  - Shader dot grid: dots every 28 units at 30 % white, fading out between 1 600 and 5 200 units from the camera.
  - Faint lines every 600 units.
- **Fog:** linear black fog. Its far distance grows as the user discovers more: 1 500 at start, about 4 000 while working, 26 000 in the hub view. The near distance is 12 % of the far.
- **Pillars and dust:**
  - Very faint pillars (3–7 % opacity, fading toward the top) scattered far away.
  - About 1 000 dust points at 40 % white.
- **Bloom:** strength 0.7, radius 0.5, threshold 0.62.

## 7. Events → visuals (what the code must emit)

The renderer should only consume typed events like these. Timings are taken from the prototype.

| Event | Payload | Visual |
|---|---|---|
| `project.open` | path, tree with line counts | Build the district. The camera starts at ground level near the project entrance (pitch 3°, distance 300). |
| `prompt.sent` | text | The ghost rises 90 units. The prompt bubble appears in the chat. |
| `step.start` | `ask` · `context` · `think` · `act` · `critic` · `answer` | The trace row turns active and the big title changes. |
| `context.load` | files[] | White beams from the ghost to each file, staggered by 0.15 s. The files glow white (0.3 s in, 0.6 s out). |
| `file.read` | path, agent | The agent flies to the file (eased, with an arc up to 140 high). White beam, white top glow. |
| `file.write` | path, agent, +lines, −lines | The file turns orange. When the change grows the file by a slab, a new slab rises 34 units and scales from 0.6 to 1 over 0.5 s. The file keeps a "changed" orange aura. |
| `file.create` | path, agent | Same as write, from 0 slabs. The folder plate gets the new file. |
| `agent.spawn` | id, name, kind, target (path or `web`) | A small ghost leaves the main ghost and flies to its target in 1.2 s (2.2 s for web). An orange beam links it to the file it builds. |
| `agent.done` | id | The ghost is **pushed off**: horizontal speed 260, initial upward speed 200, gravity 900, tumbling as it falls. It lands with an **impact** (see below), then sinks into the floor in 0.6 s. |
| `critic.start` / `critic.done` | file | Kiro's perch slab appears next to the file. On done, Kiro is pushed off and its perch fades out. |
| `checkpoint.saved` | n | Meta line in the chat. Changed files keep their aura. |
| `rewind` | to, files[] | Every slab created after the checkpoint falls. Each file's slabs fall top-first, 70 ms apart, with files staggered by 120 ms. Big impact. The camera drops 70 units, the FOV punches +8°, and the camera shakes. |
| `hub.open` / `hub.close` | — | The camera pulls back to about 9 000–12 500 units and the fog opens. Project names appear. The active project's outline turns bright orange. |

**Impact on the floor:**

- An orange ring growing to 260 (agent) or 420 (slab) over 1.4 s.
- A white ring growing to 1.5× that size, starting 0.12 s later and lasting 1.6 s.
- An orange flash that fades in 0.45 s.
- 26 dust particles thrown outward, fading in 1.2 s.
- A camera shake of 4, or 12 for big impacts, decaying in 0.9 s.

## 8. Camera

- **Field of view:** 42° by default.
- **Framing:** always orbits a target (the agent or the area being worked on) using distance, yaw and pitch, with smooth spline interpolation between shots.
- **Typical shots:**

  | Shot | Distance | Pitch |
  |---|---|---|
  | Close on the agent | 600–950 | 10–24° |
  | Project overview | 1 500–2 200 | 22–32° |
  | Hub | 9 000+ | 38° |

- **Parallax of the UI layers:** computed from the camera velocity in screen space.
  - The panel moves up to ±46 px and turns up to ±5° around Y (base tilt 4°).
  - Other layers move by a depth factor: title 1.5, caption 1.2, metrics 0.55, header 0.25.

## 9. Project list (data source)

**Recommendation: local folders, not GitHub.**

- The active project is the folder Crypt is launched in. Its tree and line counts are read from disk, so uncommitted edits show up immediately.
- Other projects come from a configurable list of workspace folders, for example `~/code`. Every sub-folder that contains a `.git` counts as a project.
- Ignore `target/`, `.git/`, `node_modules/`, build outputs and binaries.
- GitHub can be added later as an optional source (repos not cloned locally, shown greyed out), but it is not needed for v1.

## 10. Out of scope for v1

- The "giant ghost" sequence in the prototype is a cinematic idea, not a product feature.
- The chapter bar, the sliders and the 90-second scripted timeline are only part of the lab.
