export function canonicalFormat(value: string): string {
  return value.trim().toLowerCase();
}

export function legacyFormat(value: string): string {
  return value.replace(/\s+/g, "_");
}
