// Runs semantic-release from the pinned devDependency and hands the result to later
// workflow jobs, which build the installers and publish the GitHub release.
import { appendFileSync, writeFileSync } from 'node:fs';
import semanticRelease from 'semantic-release';

const result = await semanticRelease();
const next = result ? result.nextRelease : undefined;

const outputs = { published: next ? 'true' : 'false' };
if (next) {
  outputs.version = next.version;
  outputs.tag = next.gitTag;
  if (process.env.RELEASE_NOTES_FILE) {
    writeFileSync(process.env.RELEASE_NOTES_FILE, next.notes ?? '');
  }
}

if (process.env.GITHUB_OUTPUT) {
  const lines = Object.entries(outputs).map(([key, value]) => `${key}=${value}`);
  appendFileSync(process.env.GITHUB_OUTPUT, `${lines.join('\n')}\n`);
}
console.log(outputs);
