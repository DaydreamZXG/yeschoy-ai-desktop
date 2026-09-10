import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import postcss from 'postcss';

// Read-only check of the actual production artifacts, not source text alone.
const auditRoot = dirname(fileURLToPath(import.meta.url));
const results = [];
for (const bundle of ['renderer-dist', 'browser-dist']) {
  const assets = join(auditRoot, bundle, 'assets');
  const stylesheets = readdirSync(assets).filter(name => name.endsWith('.css'));
  const roots = stylesheets.map(name => ({
    name,
    source: readFileSync(join(assets, name), 'utf8'),
  }));
  const rules = roots.flatMap(({ source }) => {
    const found = [];
    postcss.parse(source).walkRules(rule => found.push(rule));
    return found;
  });
  const verified = [];
  const requireDeclaration = (selector, property, value) => {
    assert(rules.some(rule => rule.selectors.includes(selector) &&
      rule.nodes.some(node => node.type === 'decl' && node.prop === property && node.value === value)),
    `${bundle}: missing ${selector} { ${property}: ${value} }`);
    verified.push(`${selector} / ${property}: ${value}`);
  };
  requireDeclaration('.yeschoy-dialog-content', 'position', 'fixed');
  requireDeclaration('.yeschoy-dialog-overlay', 'position', 'fixed');
  requireDeclaration('.yeschoy-dialog-content', 'top', '50%');
  requireDeclaration('.yeschoy-dialog-content', 'left', '50%');
  requireDeclaration('.yeschoy-dialog-content', 'overflow-y', 'auto');
  requireDeclaration('.yeschoy-dialog-content', 'max-height', 'calc(100dvh - 32px)');
  requireDeclaration('.yeschoy-dialog-content[data-dialog-layer=alert]', '--yeschoy-dialog-layer', '60');
  requireDeclaration('.yeschoy-dialog-content[data-dialog-variant=fullscreen]', 'transform', 'none');
  requireDeclaration('.yeschoy-dialog-content.confirm-dialog', 'max-width', '420px');
  results.push({ bundle, verified, stylesheets: roots.map(({ name, source }) => ({
    name,
    sha256: createHash('sha256').update(source).digest('hex'),
  })) });
}
process.stdout.write(JSON.stringify({ passed: true, results }, null, 2) + '\n');
