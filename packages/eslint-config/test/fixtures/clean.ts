/** Idiomatic code that must pass the shared config without any report. */
export interface Member {
  readonly id: string;
  readonly name: string;
}

export function label(member: Member): string {
  return `${member.name} (${member.id})`;
}

export async function loadLabel(fetchMember: () => Promise<Member>): Promise<string> {
  const member = await fetchMember();
  return label(member);
}
