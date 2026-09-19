import type { DictationSession, PolishMode, StylePack } from './types';

export function packDisplayName(
  pack: StylePack,
  modeLabel: Record<PolishMode, string>,
): string {
  return pack.kind === 'builtin' ? modeLabel[pack.baseMode] : pack.name.trim();
}

export function defaultPackId(packs: StylePack[]): string {
  return packs.find(pack => pack.active)?.id || packs[0]?.id || '';
}

export function retryPackId(
  session: Pick<DictationSession, 'stylePackId'>,
  allPacks: StylePack[] | null,
  enabledPacks: StylePack[],
): string | undefined {
  const original = session.stylePackId
    ? allPacks?.find(pack => pack.id === session.stylePackId)
    : undefined;
  return original?.id || defaultPackId(enabledPacks) || undefined;
}
