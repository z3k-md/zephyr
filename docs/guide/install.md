---
title: Install
summary: Download Zephyr for Windows or macOS and handle first-run security prompts.
---

Download the latest installer from [Releases](https://github.com/z3k-md/zephyr/releases/latest): the `-setup.exe` for Windows, or the `.dmg` for macOS (Apple silicon). Installed copies check for updates shortly after launch and whenever the bar opens (at most every few minutes), and install them automatically. Settings and the tray menu also have **Check for updates**. See [Updates](/features/updates) for how that works.

## Windows

The Windows installer is not Authenticode-signed yet, so SmartScreen warns about an unrecognized app the first time: choose **More info** and then **Run anyway**. Updates are still verified against Zephyr's own updater key.

## macOS

The app is signed with an optional self-signed certificate when the release build has one; it is not notarized. The first time you open it, right-click the app and choose **Open**, then confirm. After that, macOS remembers your choice for this identity. Without a certificate the build is ad-hoc signed and may ask for permissions again after each update.
