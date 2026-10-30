// What typed text asks for a ticker card. Crypto by its symbol or name in any case ("eth",
// "Btc", "bitcoin"); a stock typed in capitals ("TSLA", "BRK.B"), with a $ ("$tsla"), or with a
// word after it ("tsla stock"). Plain lowercase stays a search, so "net" or "app" never turns
// into a stock card.

export interface Ticker {
  symbol: string;
  crypto: boolean;
}

/** Crypto that reads as itself in any case. */
const CRYPTO: Record<string, string> = {
  btc: 'BTC',
  bitcoin: 'BTC',
  eth: 'ETH',
  ethereum: 'ETH',
  ether: 'ETH',
  solana: 'SOL',
  xrp: 'XRP',
  doge: 'DOGE',
  dogecoin: 'DOGE',
  cardano: 'ADA',
  ltc: 'LTC',
  litecoin: 'LTC',
  bnb: 'BNB',
  polkadot: 'DOT',
  avax: 'AVAX',
  avalanche: 'AVAX',
  chainlink: 'LINK',
  shib: 'SHIB',
  xlm: 'XLM',
  stellar: 'XLM',
  usdc: 'USDC',
  usdt: 'USDT',
  tether: 'USDT',
};

/** Crypto symbols that are everyday words in lowercase, so only count in capitals or with $. */
const CRYPTO_CAPS = new Set(['SOL', 'ADA', 'DOT', 'LINK', 'TRX']);

const SYMBOL = /^[a-z]{1,5}(?:\.[a-z])?$/i;
const SUFFIX = /^(\S+)\s+(?:stock|stocks|shares|price|quote|ticker)$/i;

function asked(symbol: string, anyCase: boolean): Ticker | null {
  const lower = symbol.toLowerCase();
  if (CRYPTO[lower]) return { symbol: CRYPTO[lower], crypto: true };
  if (!SYMBOL.test(symbol)) return null;
  const upper = symbol.toUpperCase();
  if (CRYPTO_CAPS.has(upper) && (anyCase || symbol === upper)) {
    return { symbol: upper, crypto: true };
  }
  return anyCase || symbol === upper ? { symbol: upper, crypto: false } : null;
}

export function tickerFor(text: string): Ticker | null {
  const typed = text.trim();
  if (typed.startsWith('$')) return asked(typed.slice(1), true);
  const withWord = SUFFIX.exec(typed);
  if (withWord) return asked(withWord[1], true);
  // Bare capitals need two letters: a lone "A" or "I" is usually the start of something else.
  if (typed.length < 2 && !CRYPTO[typed.toLowerCase()]) return null;
  return asked(typed, false);
}
