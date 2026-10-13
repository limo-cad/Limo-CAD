/** JSON object member order has no model meaning. Arrays and every stored
 * value remain exact; never discard metadata or tolerate geometric drift. */
export function projectModelDifference(beforeJson: string, afterJson: string): string | null {
  const before: unknown = JSON.parse(beforeJson);
  const after: unknown = JSON.parse(afterJson);
  const summary = (value: unknown): string => {
    if (Array.isArray(value)) return `array(${value.length})`;
    if (value && typeof value === 'object') return `object(${Object.keys(value).length})`;
    return JSON.stringify(value)?.slice(0, 100) ?? 'missing';
  };
  const compare = (a: unknown, b: unknown, path: string): string | null => {
    if (a === b) return null;
    if (Array.isArray(a) && Array.isArray(b)) {
      if (a.length !== b.length) return `${path}.length: ${a.length} -> ${b.length}`;
      for (let i = 0; i < a.length; i++) {
        const difference = compare(a[i], b[i], `${path}[${i}]`);
        if (difference) return difference;
      }
      return null;
    }
    if (a && b && typeof a === 'object' && typeof b === 'object' && !Array.isArray(a) && !Array.isArray(b)) {
      const left = a as Record<string, unknown>;
      const right = b as Record<string, unknown>;
      const keys = [...new Set([...Object.keys(left), ...Object.keys(right)])].sort();
      for (const key of keys) {
        const child = `${path}[${JSON.stringify(key)}]`.slice(0, 500);
        if (!Object.prototype.hasOwnProperty.call(left, key)) return `${child}: missing -> ${summary(right[key])}`;
        if (!Object.prototype.hasOwnProperty.call(right, key)) return `${child}: ${summary(left[key])} -> missing`;
        const difference = compare(left[key], right[key], child);
        if (difference) return difference;
      }
      return null;
    }
    return `${path}: ${summary(a)} -> ${summary(b)}`;
  };
  return compare(before, after, '$');
}
