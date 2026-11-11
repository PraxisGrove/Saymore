# Saymore Magpie Agent plugin

Select a Magpie model in Magpie's **Agents → Saymore** row and use it for
Saymore's existing text-polishing flow. Saymore keeps a separate `magpie`
Provider; connecting does **not** replace the existing Custom Provider or any
other saved Provider.

## Requirements

- Magpie with Agent plugin support (the current `magpie.agent` JavaScript API).
- A Saymore build containing the native Magpie Provider and its startup import.
- Magpie running its local OpenAI-compatible gateway at
  `http://127.0.0.1:3425/v1`.
- macOS, using Saymore's production configuration at
  `~/Library/Application Support/Saymore/config.json`. Development profiles and
  Windows are not handled by this plugin.

## Install

This changes Magpie's installed plugins. Install only when you intend to enable
that change:

```bash
magpie plugin add ./integrations/magpie
```

## Connect and use

1. Close Saymore completely. An import happening while it runs can be
   overwritten by its normal settings saves.
2. In Magpie, choose **Agents → Saymore → model**, then select a configured
   upstream model (`provider/model`) or routing group (`group/id`).
3. Reopen Saymore. It imports the connection into its own Magpie Provider and
   removes the plugin's `integrations.magpie` handoff, retaining the credential
   only in the Provider configuration.
4. Open **Models → Text polishing → Magpie**. Refresh the model list if needed,
   choose the model, and click **Test and use** to explicitly enable polishing.
   The local attribution key supplied by Magpie normally needs no replacement.
5. Dictation text goes through Magpie only after successful connection testing
   and enablement. Magpie handles protocol translation and downstream routing;
   its upstream model may be remote and may incur that provider's charges.

Changing the external model alone never enables polishing. If the previously
active Magpie model changes, activation is cleared. The existing refinement
timeout, retry, dictionary-hint consent and output-validation rules still apply.
For a changed model while Saymore is enabled, test/enable before relying on the
new Provider.

## Disconnect

Disconnect in Magpie or uninstall this plugin to restore its original file keys.
After the one-shot startup import, that restoration removes an already-consumed
handoff; saved Saymore Provider settings are not repeatedly imported, replaced,
or automatically disabled. Manage those directly in Saymore.

Selecting a different Magpie model intentionally writes a fresh connection. It
must be tested again in Saymore before use. Use Saymore rather than Magpie to
switch back to another saved Provider.

## Checks

These execute local JavaScript tests only; they do not install the plugin or
send model requests:

```bash
node --test integrations/magpie/agent.test.mjs
```

The mock infrastructure tests cover Magpie's model directory, attribution
header, portable chat request and successful response parsing. No live model
request is implied by these checks. Hosted Magpie installation/restore and a
real polishing request require explicit acceptance testing.
