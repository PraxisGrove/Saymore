# Automatic dictionary Accessibility compatibility

Tested on macOS on 2026-07-16 with Saymore Preview. The probe inspects only
Accessibility capability metadata. It does not read, print, or persist text
from the focused control.

## Result

| Application | Tested control | Direct AX write | Range metadata | Local range read | Correction observation | Delivery strategy |
| --- | --- | --- | --- | --- | --- | --- |
| Safari 26.3.1 | Local-page `textarea` | Yes | Yes | Yes | Supported | Direct AX write, then observe |
| Google Chrome 150.0.7871.115 | Local-page `textarea` | Yes | Yes | Yes | Supported | Direct AX write, then observe |
| Visual Studio Code 1.129.0 | Untitled editor | Yes | Yes | Yes | Supported | Direct AX write, then observe |
| Microsoft Word 16.111 | Blank document editor | No | Yes | Yes | Supported | Clipboard paste fallback, verify, then observe |
| WeChat 4.1.11 | Chat editor | Not tested | Not tested | Not tested | Blocked | WeChat required account reauthentication before a chat editor could be focused |

`Correction observation: Supported` means the focused control exposes both a
selected range and `AXStringForRange`, which are the prerequisites used by the
local correction observer. It does not by itself prove the complete workflow
from dictation through two user corrections, dictionary promotion, and the
notification overlay.

Word does not expose `AXSelectedText` as settable for its document editor. This
does not block dictionary learning: the existing delivery pipeline can use its
clipboard paste fallback, verify the inserted range, and then observe later
changes through the supported range APIs.

## Preview diagnostic

The Preview build exposes a local diagnostic command:

```bash
/Applications/Saymore\ Preview.app/Contents/MacOS/saymore-desktop \
  --probe-focused-text-control <PID>
```

The command talks to the already Accessibility-authorized Preview process over
a user-only Unix socket. The result contains only:

- target application bundle identifier;
- focused control role and subrole;
- whether a selected range is available;
- whether direct selected-text writes are available;
- whether a zero-length local range can be read;
- the derived observation state: `observable`, `delivery_only`, or `sensitive`.

The server is unavailable in production builds. Password fields, Secure Event
Input, and known secure controls are classified as `sensitive` and are never
reported as observable.

## Remaining manual verification

After WeChat is signed in, focus the File Transfer Assistant message editor and
run the same probe against the main WeChat PID. For every application marked
supported above, a complete release check must still perform two independent
dictations, correct both outputs to the same final term, wait for the edit
stability window, and confirm that one dictionary entry and one notification
are produced.

Feishu and Notion were not installed and were intentionally not tested.
