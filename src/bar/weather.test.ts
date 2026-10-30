import { describe, expect, it } from 'vitest';
import type { Forecast } from '../types';
import { placeName } from './weather';

const at = (name: string, region: string) => ({ place: { name, region } }) as unknown as Forecast;

describe('placeName', () => {
  it.each([
    ['Portland', 'Oregon', 'Portland, Oregon'],
    ['Mexico City', 'Mexico City', 'Mexico City'],
    ['Berlin', 'berlin', 'Berlin'],
    ['Tokyo', '', 'Tokyo'],
  ])('%s, %s reads %s', (name, region, expected) => {
    expect(placeName(at(name, region))).toBe(expected);
  });
});
