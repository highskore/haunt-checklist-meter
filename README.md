# Checklist meter

Show a repo's spec or TODO checklist progress on the agent's Haunt row.

Agents working through a long plan tick boxes in a file such as `specs/plan.md`. This plugin counts the ticked and total Markdown checkboxes (`- [x]` / `- [ ]`, any list style, code blocks skipped) and shows them on the agent's row: `spec 11/18` on the right, the file after the meta line (`phase-3 · specs/plan.md`) and a thin bar under the row. A finished list reads `spec 18/18 ✓` and the bar goes.

It reads `specs/*.md` and `TODO.md` in the agent's folder. With several files it shows the first with unticked boxes. It counts again when a turn starts or ends and after every edit. It doesn't ask to see the agents' file paths, so it can't tell which file an edit touched.

## Settings

```toml
[plugins."dev.haunt.checklist-meter"]
files = ["specs/*.md", "TODO.md"]   # relative to the agent's folder; only these are granted
path = true                         # the file after the meta line
bar = true                          # the progress bar
unverified = false                  # also follow events without the tab's nonce
```

## Capabilities

| capability | why |
|---|---|
| `events` (session start, prompt, stop, tool, file edited) | when to count again; it sees tool names, not commands or paths |
| `fs_read = ["${cwd}/specs/*.md", "${cwd}/TODO.md"]` | the checklists, only inside the agent's folder |
| `decorate` | the count, path and bar on the row |

Haunt widens `${cwd}` only to folders from verified events, never to `$HOME` or the Haunt folder. Events without the tab's nonce are ignored unless `unverified` is on.

## Build and test

```sh
cargo build --release --target wasm32-wasip2   # or ./build.sh
cargo test                                     # unit tests
haunt plugin install .
```

`e2e/checklist_meter.rs` runs the built plugin against a real Haunt core: copy it into a Haunt checkout's `cli/tests/` and run `HAUNT_PLUGINS_DIR=<this repo's parent> cargo test -p haunt-cli --test checklist_meter`.

Needs Haunt with plugin API 0.5 (`api = "0.5"`).

## Licence

MIT, see `LICENSE`.
