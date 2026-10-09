---
title: Import & Export
sidebar_label: Import & Export
sidebar_position: 6
---

# Import & Export

Mercury imports Postman and Insomnia exports and cURL commands, and it can copy any request as a cURL command.

## Local Insomnia Cloud bridge (Linux)

Choose **Sync → Browse cloud collections…** to sign in through your browser.
Complete authentication on Insomnia's website and paste its authentication code
into Mercury. You can also use an existing installed Insomnia login. Search
collections by name or organization, select one, and choose an empty folder to
download master. No collection export or project ID is needed for this flow.

In a workspace fetched by the bridge, click the sync arrows and **master** in
the bottom bar. Choose **Pull from master** to download updates or **Push to
master** to share request changes. Mercury saves the open request, pauses editing
during sync, and refreshes the workspace afterward. Errors remain in the sync
menu. Pull refuses unpushed edits; push checks cloud conflicts.

This local fork includes a Rust bridge using Mercury's saved sign-in or the
installed Insomnia login. All
cloud reads and pushes target **master**, regardless of Insomnia's active branch.
The bridge syncs requests, their folder locations and cloud environments.
Managed `.env.insomnia-<name>-<id>.json` files preserve JSON types; sub-environments
inherit base variables. Edit these files and push to share environment changes.
Ordinary local `.env` files are never uploaded. Close Mercury while using the
terminal commands below, then reopen
its workspace. Sync from the bottom bar handles the refresh automatically.

```bash
mercury --insomnia-fetch <cloud-project-id> <workspace-folder>
mercury --insomnia-collections
mercury --insomnia-plan <cloud-project-id> <workspace-folder>
mercury --insomnia-push <cloud-project-id> <workspace-folder>
```

Fetch initially requires an empty folder. After fetching, open that folder in
Mercury and edit requests normally. Plan previews creations, updates and
deletions without uploading them. Push commits saved changes to master; teammates
select master and pull in Insomnia. `--insomnia-status <cloud-project-id>` checks
connectivity and shows master's commit and request counts.

Fetch refuses unpushed request edits and retains the previous workspace in a
sibling `.insomnia-backup_*` folder. Push refuses conflicting cloud edits. If a
push is interrupted, repeat push to reconcile the saved pending commit. Preserve
`.insomnia-sync`, which stores the baseline and recovery data.

Configure local `.env` files separately; they are retained by fetch and never
uploaded. Digest auth, scripts, multipart/file bodies and Insomnia template tags
remain in cloud documents but are not executed by Mercury. Each fetch reports
these limits in `.insomnia-sync/warnings.txt`; unsupported body/header edits are
rejected to prevent losing configuration.

The bridge uses Insomnia 13.3's internal Cloud Sync protocol. Building this fork
requires OpenSSL development libraries (on Ubuntu, `libssl-dev` and `pkg-config`).
Other platforms have not been verified. The file importer remains separate and
supports the legacy `resources` format, not Insomnia v5 collections.

## Where imports go

Choose **Import from Postman…** or **Import from Insomnia…** from the workspace menu in the top bar. The empty sidebar also has **Import from ...** links. Pick the export file, and then:

- if a workspace is open, the requests are written **into that workspace**
- if no workspace is open, Mercury asks for a folder to write to, then opens it as the workspace

A notification reports how many requests and environments were imported.

File and folder names are sanitized: they're lowercased, and spaces and characters such as `/ \ : * ? " < > |` become `-`. For example, "Get User" becomes `get-user.json`. Existing files with the same name are **overwritten**.

## Postman

Export the collection from Postman as **Collection v2.1** (JSON).

| Postman | Mercury |
|---------|---------|
| Request | `<name>.json` |
| Folder (any depth) | Folder, with the same nesting |
| Collection variables | `.env.<collection-name>` in the import folder |
| Headers | Headers (disabled ones are skipped) |
| Raw body | Body |
| URL-encoded body | A [Form body](/docs/features/requests#body), with the matching `Content-Type` |
| Bearer / Basic / API key auth | The `Authorization` (or API key) header, or a query parameter |
| URL | The raw URL. Without one, the URL is rebuilt from its parts, skipping disabled query params. |

Auth set on the collection or a folder applies to every request under it, and a request's own auth wins over it.

Postman `{{variables}}` stay as they are in URLs, headers and bodies, so they work with the generated `.env` file. Scripts and tests aren't imported.

What Mercury can't express is **named in the notification** rather than dropped quietly: OAuth 2 and other token-exchange auth, `form-data` and binary bodies, which need a file a request file cannot point at, and GraphQL bodies. Anything listed there needs finishing by hand on the [Auth tab](/docs/features/auth) or the Body tab.

Example: importing a collection named "My API":

```
workspace/
├── .env.my-api
├── auth/
│   └── login.json
├── users/
│   └── admin/
│       └── list-admins.json
└── health-check.json
```

## Insomnia

Export from Insomnia as JSON or YAML.

| Insomnia | Mercury |
|----------|---------|
| Request inside a request group | `<group-name>/<name>.json` |
| Request not in a group | `imported/<name>.json` |
| Environment (non-empty) | `.env.<environment-name>` |
| Headers | Headers (disabled ones are skipped) |
| Body text | Body |

Each request goes into a folder named after its **direct parent** group. Nested groups aren't recreated as nested folders. Auth settings aren't imported.

```
workspace/
├── .env.base-environment
├── users/
│   ├── get-user.json
│   └── list-users.json
└── imported/
    └── health.json
```

## Import from cURL

Paste a command that starts with `curl ` into the **URL bar**. Mercury parses it and replaces the method, URL, headers and body. This works with commands copied from browser DevTools or API docs, including multi-line commands that use `\` continuations.

```bash
curl -X POST https://api.example.com/users \
  -H "Content-Type: application/json" \
  -d '{"name": "John"}'
```

| Flag | Effect |
|------|--------|
| `-X`, `--request` | Method |
| `-H`, `--header` | Header |
| `-d`, `--data`, `--data-raw`, `--data-binary` | Body. Changes a `GET` to `POST`. |
| `--json` | Body plus `Content-Type: application/json`. Changes a `GET` to `POST`. |
| `-u`, `--user` | `Authorization: Basic ...` |
| `-A`, `--user-agent` | `User-Agent` header |
| `-b`, `--cookie` | `Cookie` header |
| `-I`, `--head` | `HEAD` |
| `-G`, `--get` | `GET` |
| `-o`, `-x`, `-c`, `-m`, `-w`, `-e`, `--output`, `--proxy`, `--cookie-jar`, `--connect-timeout`, `--max-time`, `--write-out`, `--cacert`, `--cert`, `--key`, `--referer` | Ignored, along with their argument |
| Other flags (`-s`, `-L`, `-k`, `-v`, `--compressed`, ...) | Ignored |

The first argument that isn't a flag is used as the URL. If there isn't one, you get an error.

## Copy as cURL

Press `⌘ Shift C` to copy the current request to the clipboard as a cURL command:

```bash
curl -X GET 'https://api.example.com/users' \
  -H 'Accept: application/json' \
  -H 'Authorization: Bearer abc123'
```

- Variables from the selected environment are substituted in. Undefined ones stay as `{{NAME}}`.
- Disabled (`#`) headers are left out.
- Arguments are single-quoted, so you can paste the command into a POSIX shell as is.

## Sharing request files

Requests are plain `.json` files, so you can also share them by copying them, committing them to Git, or sending them to someone. See [File Format](/docs/reference/file-format).

## Related

- [Collections](/docs/features/collections)
- [Environments](/docs/features/environments)
- [File Format](/docs/reference/file-format)
