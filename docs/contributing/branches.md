# Branches

Work lands on `develop`. Checks run on Windows and macOS for every push to `develop` and every pull request to `develop` or `main`, next to the release script tests and a lint of the workflow files. A running check on `develop` always finishes; of the pushes that arrive meanwhile, only the newest is checked next, against the last commit whose checks passed, so nothing goes unchecked. Like `scripts/check.sh`, a push skips the Rust or frontend half when it doesn't touch it, and the platform-independent checks (formatting, vue-tsc, frontend tests) run on macOS only. Windows builds on a Dev Drive. CodeQL scans `develop` on each push (Rust weekly), and the Prune caches workflow keeps only the newest cargo cache per job and OS. Pushes to `main` aren't checked again: `main` moves only when `develop` is promoted, by fast-forwarding it once `develop`'s checks are green:

```bash
git push origin develop:main
```

`main` requires both checks to pass and can't be deleted. Dependabot opens version updates as pull requests to `develop`.
