import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import postcss from 'postcss';

const dir = 'outputs/model-picker-audit-20260910.YFZJny';
const html = fs.readFileSync(path.join(dir, 'renderer-dist/index.html'), 'utf8');
const files = [...html.matchAll(/(?:href|src)="(?:\.)?(\/assets\/[^"]+)"/g)]
  .map(m => 'renderer-dist' + m[1]);
const cssFile = files.find(p => p.endsWith('.css'));
if (!cssFile) throw new Error('Missing renderer stylesheet');
const css = postcss.parse(fs.readFileSync(path.join(dir, cssFile), 'utf8'));
const selectors = ['.model-picker-popover', '.model-options', '.yeschoy-dialog-content'];
const rules = {};
css.walkRules(rule => {
  for (const selector of selectors) {
    if (!rule.selector.split(',').includes(selector)) continue;
    rules[selector] ??= {};
    rule.walkDecls(d => { rules[selector][d.prop] = d.value; });
  }
});
if (rules['.model-picker-popover']?.display !== 'flex'
  || rules['.model-picker-popover']?.['max-height'] !== 'var(--radix-popover-content-available-height)'
  || rules['.model-options']?.['overflow-y'] !== 'auto'
  || rules['.model-options']?.['min-height'] !== '0'
  || rules['.yeschoy-dialog-content']?.position !== 'fixed') {
  throw new Error('Production CSS regression');
}
const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const sources = ['src/configuration/ModelPicker.tsx', 'src/configuration/ModelPicker.test.tsx',
  'src/model-profiles/profile.ts', 'src/model-profiles/profile.test.ts', 'src/workbench/workbench-v2.css'];
const prior = {
  'src-tauri/src/claude_bridge.rs': 'a6397d875abfa410896292d42c936bd003cbbed25efcf7ed23493076ffb1616b',
  'src-tauri/src/tool_activation.rs': 'cf365b54e16ec71b013b4c06f445d9a035606fd0805cfc9b69dec77a521d03c5',
  'src-tauri/src/tool_adapters/claude_desktop.rs': '955d31ecd4c42e2d9f3e3001dcccb6cf8e1b70ad600ce430b49030045f967890',
  'src-tauri/src/tool_adapters/codex_desktop.rs': 'eb7f5741969f668c8e33aea92e2b1202f2df7d0fc6797579025ec781822e19f2',
  'src-tauri/src/tool_model_profile.rs': '42136c34da125c3c852de4d940e2579188bd2a2f82e76952bb30906c041772a0',
  'src/model-profiles/catalog.json': '882bc2b22556fa26a1419450f77a44358eecfc1f9709fbcde75da2504fc6eeaf',
  'src/configuration/connections.tsx': '3a64551d71376c72567543730346ef65edef15226e9604c6bfd127f424179fdc',
  'src/configuration/connections.test.tsx': 'a36e259b14dd24ac27d2dee83addd47557423344b0519573e64b6a9e5436da4a',
};
const preserved = Object.fromEntries(Object.entries(prior).map(([p, hash]) => [p, sha(p) === hash]));
if (Object.values(preserved).some(value => !value)) throw new Error('Earlier changes drifted');
const report = JSON.parse(fs.readFileSync(path.join(dir, 'related-regression-final-2.json'), 'utf8'));
console.log(JSON.stringify({
  checkedAt: new Date().toISOString(),
  sourceSha256: Object.fromEntries(sources.map(p => [p, sha(p)])),
  rendererAssetsSha256: Object.fromEntries(files.map(p => [p, sha(path.join(dir, p))])),
  productionCSS: rules,
  priorChangesPreserved: preserved,
  regression: { success: report.success, total: report.numTotalTests, passed: report.numPassedTests,
    failed: report.numFailedTests, pending: report.numPendingTests },
}, null, 2));
