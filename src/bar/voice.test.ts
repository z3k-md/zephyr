import { describe, expect, it } from 'vitest';
import { parseVoice } from './voice';

describe('parseVoice', () => {
  it('strips end punctuation, collapses spaces, and keeps the remaining words', () => {
    expect(parseVoice('  Open   Firefox.  ')).toEqual({ query: 'Firefox', action: 'top' });
    expect(parseVoice('Hello, world.')).toEqual({ query: 'Hello, world', action: 'type' });
    expect(parseVoice('Wow!!')).toEqual({ query: 'Wow', action: 'type' });
    expect(parseVoice('...Firefox')).toEqual({ query: 'Firefox', action: 'type' });
    expect(parseVoice('...')).toEqual({ query: '', action: 'type' });
    expect(parseVoice('')).toEqual({ query: '', action: 'type' });
  });

  it('only fills the box when no command word was said', () => {
    expect(parseVoice('Firefox.')).toEqual({ query: 'Firefox', action: 'type' });
    expect(parseVoice('flights to denver')).toEqual({ query: 'flights to denver', action: 'type' });
  });

  it.each([
    ['Open Firefox.', 'Firefox'],
    ['open firefox', 'firefox'],
    ['open, Firefox', 'Firefox'],
    ['firefox open', 'firefox'],
    ['firefox open.', 'firefox'],
    ['the store go', 'the store'],
    ['the dragon enter', 'the dragon'],
    ['file budget open', '!f budget'],
    ['open file budget', '!f budget'],
    ['Open Files Budget.', '!f Budget'],
  ])('open / go / enter runs the top result: %s', (text, query) => {
    expect(parseVoice(text)).toEqual({ query, action: 'top' });
  });

  it('does not treat go or enter as a leading verb', () => {
    expect(parseVoice('go to the store')).toEqual({ query: 'go to the store', action: 'type' });
    expect(parseVoice('enter the dragon')).toEqual({ query: 'enter the dragon', action: 'type' });
  });

  it('leaves a lone command word as plain text', () => {
    for (const word of ['search', 'Search.', 'open', 'google', 'ask', 'go', 'enter']) {
      const parsed = parseVoice(word);
      expect(parsed.action).toBe('type');
      expect(parsed.query.toLowerCase().replace(/[.,!?]/g, '')).toBe(
        word.toLowerCase().replace(/[.,!?]/g, '')
      );
    }
  });

  it.each([
    ['search flights to denver', 'flights to denver'],
    ['Search Flights to Denver.', 'Flights to Denver'],
    ['google cats', 'cats'],
    ['google X', 'X'],
    ['flights to denver search', 'flights to denver'],
    ['flights google', 'flights'],
    ['I want to search', 'I want to'],
    ['firefox, search', 'firefox'],
    ['files budget search', '!f budget'],
    ['search bookmarks zephyr', '!bm zephyr'],
    ['Side effects of tirzepatide? Search.', 'Side effects of tirzepatide'],
  ])('search / google: %s', (text, query) => {
    expect(parseVoice(text)).toEqual({ query, action: 'search' });
  });

  it.each([
    ['ask how do magnets work', 'how do magnets work'],
    ['how do magnets work ask', 'how do magnets work'],
    ['Ask how do magnets work?', 'how do magnets work'],
    ['What is the mechanism of apixaban? Ask.', 'What is the mechanism of apixaban'],
    ['Ask. What is the mechanism of apixaban?', 'What is the mechanism of apixaban'],
  ])('ask: %s', (text, query) => {
    expect(parseVoice(text)).toEqual({ query, action: 'ask' });
  });

  it.each([
    ['cancel', ''],
    ['Cancel.', ''],
    ['never mind', ''],
    ['Never mind!', ''],
    ['nevermind', ''],
    ['Nevermind', ''],
    ['firefox cancel', 'firefox'],
    ['firefox nevermind', 'firefox'],
    ['please never mind', 'please'],
    ['open firefox, never mind.', 'open firefox'],
    ['open the pod bay never mind', 'open the pod bay'],
  ])('cancel: %s', (text, query) => {
    expect(parseVoice(text)).toEqual({ query, action: 'cancel' });
  });

  it('does not treat a leading cancel as the command, so "cancel shutdown" still works', () => {
    expect(parseVoice('cancel shutdown')).toEqual({ query: 'cancel shutdown', action: 'type' });
    expect(parseVoice('cancel the meeting')).toEqual({
      query: 'cancel the meeting',
      action: 'type',
    });
    expect(parseVoice('never mind firefox')).toEqual({
      query: 'never mind firefox',
      action: 'type',
    });
  });

  it('ignores a command word inside another word or mid-sentence', () => {
    expect(parseVoice('research paper')).toEqual({ query: 'research paper', action: 'type' });
    expect(parseVoice('research')).toEqual({ query: 'research', action: 'type' });
    expect(parseVoice('searching for cats')).toEqual({
      query: 'searching for cats',
      action: 'type',
    });
    expect(parseVoice('googling cats')).toEqual({ query: 'googling cats', action: 'type' });
    expect(parseVoice('opening firefox')).toEqual({ query: 'opening firefox', action: 'type' });
    expect(parseVoice('reopen the file')).toEqual({ query: 'reopen the file', action: 'type' });
    expect(parseVoice('please search flights')).toEqual({
      query: 'please search flights',
      action: 'type',
    });
    expect(parseVoice('clipart show')).toEqual({ query: 'clipart show', action: 'type' });
  });

  it.each([
    ['file budget', '!f budget'],
    ['files my taxes', '!f my taxes'],
    ['File Budget Report', '!f Budget Report'],
    ['bookmark github', '!bm github'],
    ['bookmarks work docs', '!bm work docs'],
    ['setting bluetooth', '!set bluetooth'],
    ['settings display', '!set display'],
    ['app Firefox', '!app Firefox'],
    ['apps Chrome', '!app Chrome'],
    ['clipboard hello', '!clip hello'],
    ['clip that', '!clip that'],
    ['note shopping list', '!note shopping list'],
    ['emoji fire', '!emoji fire'],
    ['shell git status', '> git status'],
    ['SHELL git status.', '> git status'],
    ['terminal ls', '> ls'],
  ])('leading scope word: %s', (text, query) => {
    expect(parseVoice(text)).toEqual({ query, action: 'type' });
  });

  it('only scopes a word at the start, and only when more words follow', () => {
    for (const word of ['files', 'file', 'note', 'shell', 'terminal', 'apps', 'emoji']) {
      expect(parseVoice(word)).toEqual({ query: word, action: 'type' });
    }
    expect(parseVoice('notes hello')).toEqual({ query: 'notes hello', action: 'type' });
    expect(parseVoice('the file budget')).toEqual({ query: 'the file budget', action: 'type' });
    expect(parseVoice('apps files photos')).toEqual({ query: '!app files photos', action: 'type' });
    expect(parseVoice('terminal.')).toEqual({ query: 'terminal', action: 'type' });
  });

  it('takes one command, preferring the trailing word', () => {
    expect(parseVoice('search open')).toEqual({ query: 'search', action: 'top' });
    expect(parseVoice('open search flights')).toEqual({ query: 'search flights', action: 'top' });
    expect(parseVoice('google ask')).toEqual({ query: 'google', action: 'ask' });
  });

  it('drops a filler from the breath before the words', () => {
    expect(parseVoice('Hmm. Open Firefox.')).toEqual({ query: 'Firefox', action: 'top' });
    expect(parseVoice('Um... search flights')).toEqual({ query: 'flights', action: 'search' });
    for (const filler of ['Okay.', 'Mm.', 'Hmm', 'uh']) {
      expect(parseVoice(filler)).toEqual({ query: '', action: 'type' });
    }
    expect(parseVoice('Mmm whatcha say')).toEqual({ query: 'whatcha say', action: 'type' });
  });

  it('writes times the way the power commands read them', () => {
    expect(parseVoice('Restart at 11 p.m.')).toEqual({ query: 'Restart at 11 pm', action: 'type' });
    expect(parseVoice('Shut down at 7:30 A.M.')).toEqual({
      query: 'Shut down at 7:30 am',
      action: 'type',
    });
    expect(parseVoice('Shut down in 30 minutes.')).toEqual({
      query: 'Shut down in 30 minutes',
      action: 'type',
    });
  });
});
