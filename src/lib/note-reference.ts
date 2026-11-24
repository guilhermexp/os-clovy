export function noteReferenceToken(ref: { id: string; title: string }): string {
  const title = ref.title.replace(/\s+/g, " ").replaceAll('"', "").trim().slice(0, 80);
  return title ? `@note:${ref.id} ("${title}")` : `@note:${ref.id}`;
}
