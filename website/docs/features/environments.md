---
title: Environment Variables
sidebar_label: Environments
sidebar_position: 3
---

# Environment Variables

Environments are `.env` files in the workspace root. Use them for base URLs, tokens and IDs that change between setups, and to keep secrets out of request files.

## Environment files

Every file in the **workspace root** whose name starts with `.env` is an environment: `.env`, `.env.dev`, `.env.production`, `.env.local` and so on. `.env` files in subfolders are ignored.

```bash
# .env.dev
BASE_URL=http://localhost:3000
API_TOKEN=dev-token

# .env.production
BASE_URL=https://api.example.com
API_TOKEN="prod token"
```

## Choosing an environment

**Only one environment is active at a time.** Ordinary dotenv files aren't layered or merged. A plain `.env` is not loaded on top of the selected one.

- Pick the environment in the menu at the **top right** of the window. **None** stops loading environment files; request session variables can still be used.
- `⌘ E` switches to the next environment. After the last one it goes to None, then starts over.
- When you open a workspace, Venus selects the first file (alphabetically) whose name contains `.dev`, or else the first file. When Venus reopens the workspace at launch, it restores the environment you last had selected.
- Environment names that contain `prod` are shown in red, and names that contain `stag` in amber, so it's harder to send to production by mistake.

## Insomnia Cloud environments

The Cloud bridge downloads managed `.env.insomnia-<name>-<id>.json` files. Select
them in the same menu. These files contain JSON objects, preserving native value
types and multiline strings. Sub-environments inherit their base environment.
Edit a managed file using its pencil in the environment menu, then **Sync → Push to master** to share
its changes. Local dotenv files are never uploaded. Keep managed filenames intact;
adding and renaming cloud environments is not supported yet.

## File format

```bash
# Full-line comments start with #
BASE_URL=https://api.example.com
USER_ID = 12345
QUOTED="two words"
SINGLE='also fine'
EQUALS=a=b
```

- One `KEY=VALUE` per line. The line is split at the first `=`.
- Spaces around the key and value are trimmed.
- A value wrapped in matching `"` or `'` has the quotes removed. No other escaping is done.
- Blank lines and lines starting with `#` are ignored. Lines without `=` are ignored.
- Keys are case-sensitive.
- Values are used as they are. A value that contains `{{OTHER}}` is not expanded.

## Using variables

Write `{{NAME}}` in the URL, a header (including the Auth tab) or the body:

```
GET {{BASE_URL}}/users/{{USER_ID}}
Authorization: Bearer {{API_TOKEN}}
```

- Variables are substituted when you send a request or copy it as cURL. Request files always keep the `{{NAME}}` placeholders.
- Spaces inside the braces are allowed: `{{ NAME }}` is the same as `{{NAME}}`.
- A variable the active environment doesn't define is **left as-is** and sent as the literal text `{{NAME}}`.

## Session variables

The request editor's [Variables tab](/docs/features/requests#before-and-after-a-request)
can set values before sending and capture JSON fields after a successful response.
These values override loaded environment values in memory. Each selected
environment, including **None**, has its own session values; switching back
restores them. Reloading an environment or pulling from cloud keeps those overrides.

Use **Clear session variables** in the tab to restore the selected environment's
file values. Restarting Venus or changing workspaces clears all session values.
Captured values are never saved to environment files or included in a cloud push.

## Seeing what's missing

- If any variable in the request is undefined, the URL bar gets an **amber border**. Hover over it to see the list.
- The **Params** and **Headers** tabs show a chip for each variable they use: ✓ if it's defined, ✗ if it isn't.

## Editing environment files

Click the **pencil** beside an environment in the top-right menu to select it and open its key/value editor. Use **Bulk edit** for text editing, then **Save environment** (or `⌘ S`) to save and return to the request. **Cancel** discards your edits. If the file changes on disk while the pane is open, cancel and reopen it before saving.

Managed cloud environments use JSON values in the editor: `"text"`, `42`, `true`, arrays or objects. Disabling a cloud row removes that key on save. Session variable overrides still take precedence over saved file values.

Edit `.env` files in any editor. Venus picks up changes right away: new or removed env files update the picker (your selection is kept), and edited values apply to the next request.

## Keeping secrets out of Git

```gitignore
.env*
!.env.example
```

Commit a `.env.example` with placeholder values so that others can copy it:

```bash
cp .env.example .env.dev
```

## Troubleshooting

If a variable isn't substituted:

1. Check that the right environment is selected at the top right.
2. Check that the file is in the workspace **root** and its name starts with `.env`.
3. Check the spelling. Names are case-sensitive.
4. If you just edited the file, select the environment again to reload it.

## Related

- [Requests](/docs/features/requests)
- [Authentication](/docs/features/auth)
- [Import & Export](/docs/features/import-export): Postman and Insomnia variables become `.env.<name>` files
