/** Whether `hostname` is a listed site or a subdomain of one. */
export function hostBlocked(hostname: string, sites: readonly string[]): boolean;

/** Tabs whose address is a blocked host. `now` is milliseconds. */
export function tabsToBlock(
  tabs: readonly { id?: number; url?: string }[],
  block: { sites?: readonly string[]; until?: number },
  now: number
): { id: number; host: string }[];
