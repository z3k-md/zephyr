import { describe, expect, it } from 'vitest';
import { tickerFor } from './ticker';

describe('tickerFor', () => {
  it.each([
    ['ETH', 'ETH', true],
    ['eth', 'ETH', true],
    ['Btc', 'BTC', true],
    ['bitcoin', 'BTC', true],
    ['  doge ', 'DOGE', true],
    ['SOL', 'SOL', true],
    ['$link', 'LINK', true],
    ['TSLA', 'TSLA', false],
    ['BRK.B', 'BRK.B', false],
    ['$tsla', 'TSLA', false],
    ['aapl stock', 'AAPL', false],
    ['nvda price', 'NVDA', false],
    ['ON', 'ON', false],
  ])('%s is %s', (text, symbol, crypto) => {
    expect(tickerFor(text)).toEqual({ symbol, crypto });
  });

  it.each(['tsla', 'net', 'app', 'link', 'sol', 'dot', 'A', 'chrome', 'Chrome', 'TSLAXX', '$', ''])(
    '%s is not a ticker',
    (text) => {
      expect(tickerFor(text)).toBeNull();
    }
  );
});
