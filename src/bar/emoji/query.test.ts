import { describe, expect, it } from 'vitest';
import { colonEmojiQuery, isEmojiColon, takeEmojiScope } from './query';

describe('takeEmojiScope', () => {
  it('reads !emoji before or after the text', () => {
    expect(takeEmojiScope('!emoji')).toEqual({ query: '' });
    expect(takeEmojiScope('!emoji fire')).toEqual({ query: 'fire' });
    expect(takeEmojiScope('fire !emoji')).toEqual({ query: 'fire' });
    expect(takeEmojiScope('  !Emoji   arrow  ')).toEqual({ query: 'arrow' });
  });

  it('leaves text that is not the emoji scope', () => {
    expect(takeEmojiScope('fire')).toBeNull();
    expect(takeEmojiScope('!emojis')).toBeNull();
    expect(takeEmojiScope(':fire')).toBeNull();
    expect(takeEmojiScope('!f fire')).toBeNull();
  });
});

describe('colonEmojiQuery', () => {
  it('is the text after a colon and a letter', () => {
    expect(colonEmojiQuery(':fire')).toBe('fire');
    expect(colonEmojiQuery(':arrow')).toBe('arrow');
    expect(colonEmojiQuery(':Fire')).toBe('Fire');
    expect(colonEmojiQuery('  :wave  ')).toBe('wave');
    expect(isEmojiColon(':a')).toBe(true);
  });

  it('leaves a port lookup alone', () => {
    expect(colonEmojiQuery(':3000')).toBeNull();
    expect(colonEmojiQuery(':0')).toBeNull();
    expect(colonEmojiQuery(' :8080 ')).toBeNull();
    expect(colonEmojiQuery(':3')).toBeNull();
    expect(isEmojiColon(':3000')).toBe(false);
    expect(colonEmojiQuery(':')).toBeNull();
    expect(colonEmojiQuery('fire')).toBeNull();
  });
});
