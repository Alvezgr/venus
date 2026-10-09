# Insomnia Cloud Sync

This fork includes a Rust bridge to Insomnia's existing Cloud Sync on Linux.
It downloads collection requests and environments from **master** and pushes changes
back to **master**. Exporting a collection from Insomnia is not required.
Mercury's source repository is independent
of the collection's cloud branch.

## Sign in and choose a collection

Open **Sync → Browse cloud collections…** in Mercury's bottom-left status bar.
Choose **Sign in with browser**, finish authentication on Insomnia's website,
then copy the authentication code into Mercury and click **Complete sign-in**.
Mercury uses Insomnia's encrypted code flow; passwords are entered only on the
official website. Keep Mercury open during sign-in. Each sign-in window uses a
new key and expires after 15 minutes; codes from other windows are rejected.

If Insomnia is already installed and signed in, Mercury can use that session.
Choose **Use Insomnia login** to connect or recover with it explicitly. No export
or manually copied project ID is needed for the picker. Search collections by
name or organization, select one, and choose an empty local folder. Mercury
downloads master and opens the workspace automatically.

Mercury saves its session under `~/.mercury/insomnia/session.json` (or under
`MERCURY_HOME`), outside the collection workspace. On Linux the directory uses
permissions 700 and the file uses 600. This cache contains credentials and
encryption keys; it is protected by filesystem permissions, not OS keychain
encryption. **Sign out** clears Mercury's saved credentials and prevents automatic
reuse of the installed app's session; it does not sign out the Insomnia app.

## Terminal commands

Build dependencies include OpenSSL development libraries (`libssl-dev` and
`pkg-config` on Ubuntu). The bridge is entirely Rust with no JavaScript runtime.

Close Mercury before using the terminal commands:

```bash
cargo build
./target/debug/mercury --insomnia-status <cloud-project-id>
./target/debug/mercury --insomnia-collections
./target/debug/mercury --insomnia-fetch <cloud-project-id> <workspace-folder>
./target/debug/mercury --insomnia-plan <cloud-project-id> <workspace-folder>
./target/debug/mercury --insomnia-push <cloud-project-id> <workspace-folder> "Describe changes"
```

The optional CLI project ID starts with `prj_`; `--insomnia-collections` lists
accessible collection names and IDs. An initial fetch requires an empty destination.
Open that folder as a workspace in Mercury afterward. Cloud environments are downloaded automatically; local `.env` files remain local.

## Sync inside Mercury

Click the sync arrows in the bottom-left status bar. Before opening a synced
workspace, the control shows **Sync** and offers **Open synced workspace…**.
Once connected, it shows **master**, with these actions:

- **Pull from master** downloads collection updates.
- **Push to master** opens a preview of saved request and environment changes.
  Enter a commit message, then confirm **Push to master** to share them with the team.
  The first line is the cloud commit title; remaining lines become its description.
  All saved changes are included. A blank message disables push (maximum 4096 bytes).
  If collection files change after review, choose **Review again** before pushing.

Mercury saves the open request first, pauses editing during sync, and refreshes
the workspace and file watcher afterward. Errors remain in the sync menu.
Teammates select master and pull using Insomnia's normal Cloud Sync controls.

The CLI preview command shows proposed edits without uploading them. Request
edits, renames, moves, creations and deletions are supported. Duplicating a
request clears its cloud identity so the next push creates a new request.
Empty folder edits are outside the sync scope.

The CLI push message is optional; omitting it uses the default Mercury message.
Interrupted pushes resume with their original message and saved changes, using
**Resume previous push** in the dialog.

## Cloud environments

Pull an existing synced workspace once to download its environments. Select an
environment in Mercury's top-right menu. Cloud files are named
`.env.insomnia-<name>-<id>.json` and contain the environment's own JSON data;
sub-environments inherit their parent's variables at runtime. JSON keeps value
types, objects, quotes and multiline secrets intact. Edit these files in your
text editor and choose **Push to master** to share changes. Deleting a managed
file deletes that cloud environment; a parent with remaining children cannot
be deleted, and the collection's base environment cannot be deleted. Adding or
renaming cloud environments is not supported yet.

Ordinary `.env` files stay local and are never uploaded. Keep managed filenames
and the sync baseline intact. Insomnia template tags and recursive variable
expansion retain the execution limits described below.

## Safety and recovery

- Fetch refuses unpushed edits and retains the previous workspace in a sibling
  `.insomnia-backup_*` directory. Untracked local `.env` files are retained.
- Push compares edits with a fresh master snapshot and rejects conflicts. Keep
  the edited file outside the workspace, restore its fetched version, fetch
  master, then reapply the edit to resolve a conflict.
- Keep `.insomnia-sync/baseline.json` and request `insomnia_id` fields intact.
  The baseline preserves original cloud configuration, including unsupported
  settings. Collection files, baselines and backups may contain secrets: keep
  them out of source control. Add your chosen workspace folder to `.gitignore`.
- Interrupted pushes retain recovery data. Repeat push to reconcile a commit
  already accepted by the cloud. If master advanced before acceptance, retry
  after the rejected plan is archived locally.
- A file lock prevents simultaneous sync operations. Network responses are
  bounded, use a 30-second timeout, and do not follow redirects.

## Limits and verification

Digest auth, scripts, multipart/file bodies, some path parameters and Insomnia
template tags are preserved in cloud documents but are not executed by Mercury.
Simple `{{ _.name }}` variables are projected to Mercury's `{{name}}` syntax and
translated back when edited. Unsupported multipart/file body edits and
duplicate-header edits are rejected. See `.insomnia-sync/warnings.txt` after
fetching for collection-specific execution limits.

Tests cover encryption authentication and hashes, stable request identities,
CRUD and folder moves, conflict detection, backups, local environment retention,
interrupted pushes, UI saving, and sync completion. A live URL edit was verified
through a fresh cloud fetch, native Insomnia pull, and teammate visibility;
unrelated resources and environments remained identical.

The protocol follows the installed Insomnia 13.3 client rather than a documented
public sync API. Future protocol changes may require bridge updates. Other
platforms have not been verified.
