// Validate before the common compiler/export prelude. A focused run still executes
// that prelude and never replaces the default all-family delivery check.
export function selectWorkflows(workflows, requestedValue) {
  if (requestedValue === undefined) return workflows;
  const requested = requestedValue.split(',').map(id => id.trim());
  const known = new Set(workflows.map(([id]) => id));
  if (requested.some(id => !known.has(id)) || new Set(requested).size !== requested.length) {
    throw new Error(`NEOFLASH_WORKFLOWS requires unique comma-separated workflow IDs. Available: ${[...known].join(',')}`);
  }
  const included = new Set(requested);
  return workflows.filter(([id]) => included.has(id));
}
