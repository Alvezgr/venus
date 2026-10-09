---
title: FAQ
sidebar_label: FAQ
sidebar_position: 99
---

# FAQ

## General

### What is Venus?

Venus is a desktop API client written in Rust with egui. It lets Insomnia users work with their cloud collections without running Insomnia, and stores requests as plain `.json` files in a folder you choose.

### Is it free?

Yes. Venus is MIT-licensed and open source. Venus has no separate account; cloud sync uses your existing Insomnia account. The source is at [github.com/alvezgr/venus](https://github.com/alvezgr/venus).

### Which platforms are supported?

The native app builds for macOS, Linux and Windows. The Insomnia bridge has been verified on Linux; other platforms need verification. See [Installation](/docs/getting-started#installation).

### Where is my data stored?

- **Requests and environments** are in your workspace folder, as `.json` and `.env*` files.
- **App data** is in `~/.venus/`: `state.json` (the last session), `recent.json` (unsaved requests you've sent) and `history.json` (request history). Set the `VENUS_HOME` environment variable to use a different directory.

Venus sends API requests to the endpoints you choose. Browser sign-in and collection discovery connect to Insomnia; explicit pushes upload saved collection and environment changes to Insomnia Cloud. Ordinary dotenv files stay local.

Missing app state and login files are copied from `~/.mercury/` on first read. Existing Venus files are never overwritten; `MERCURY_HOME` remains an isolated-profile compatibility alias.

### Are there settings?

No. The timeout (30 s), redirect handling and response size limits are fixed. See [Defaults](/docs/features/requests#defaults).

## Migration

### How do I import from Postman or Insomnia?

Use **Import from Postman…** or **Import from Insomnia…** in the workspace menu (the folder chip in the top bar). See [Import & Export](/docs/features/import-export) for what gets imported and where the files go.

### How do I get requests out of Venus?

Request files are plain JSON, so you can copy them, commit them or send them to someone. To share one request, press `⌘ Shift C` to copy it as a cURL command.

## Requests

### My request doesn't send

- The URL needs to include `http://` or `https://`.
- If a message says "Request timed out after 30s", "Connection failed" or "SSL/TLS error", check the network, VPN or proxy.
- Press `Esc` to cancel a request that is stuck.

### My `{{variable}}` is sent literally

The selected environment doesn't define it. Check the environment picker at the top right, and check that the file is in the workspace root. Only one environment is active at a time. See [Environments](/docs/features/environments#troubleshooting).

### How do I send form data?

Set the header and write the body yourself:

```
Content-Type: application/x-www-form-urlencoded
```
```
username=john&password=secret
```

### Can I upload files (multipart)?

No. Multipart bodies aren't supported.

### Does Venus support GraphQL?

You can send GraphQL as a normal `POST` with a JSON body and `Content-Type: application/json`:

```json
{ "query": "{ users { id name } }" }
```

There's no schema explorer.

### WebSockets, gRPC?

Not supported. Venus only sends HTTP requests.

### Are cookies kept?

Yes, for the current session only. See [Cookies](/docs/features/requests#cookies).

## Troubleshooting

### Venus won't start

- **macOS:** if Gatekeeper blocks Venus, open **System Settings → Privacy & Security** and click **Allow Anyway**.
- **Windows:** at the SmartScreen prompt, click **More info**, then **Run anyway**.

### Edits from my editor don't show up

Venus reloads the open request when its file changes, **unless you have unsaved edits in Venus** (shown by a dot after the name in the breadcrumb). In that case Venus keeps its own version. For edits to `.env` values, select the environment again in the picker.

### A large response isn't displayed

Text bodies over 148 KB aren't shown inline. Click **Save** and open the file in an editor. Responses that declare a `Content-Length` over 10 MB aren't downloaded. See [Performance](/docs/performance#response-size-limits).

## Contributing

Bug reports and pull requests are welcome on [GitHub](https://github.com/alvezgr/venus/issues). **Report an issue** in the `⋯` menu opens the issue tracker. When you report a bug, include your Venus version, your OS, the steps to reproduce, and what you expected to happen.
