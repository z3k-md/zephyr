---
title: Install
summary: Download the installer and get past the first-launch prompts.
---

Download from [Releases](https://github.com/z3k-md/zephyr/releases/latest): `-setup.exe` for Windows, `.dmg` for macOS (Apple silicon only). Updates install automatically; see [Updates](/features/updates).

## Windows

The installer isn't Authenticode-signed yet, so SmartScreen warns the first time. Choose **More info** > **Run anyway**. Updates are still verified against Zephyr's updater key.

## macOS

The app isn't notarized. The first time, right-click it, choose **Open**, and confirm. Releases built without the signing certificate are ad-hoc signed and ask for permissions again after each update.
