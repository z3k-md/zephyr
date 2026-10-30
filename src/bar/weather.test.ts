import { describe, expect, it } from 'vitest';
import type { Forecast } from '../types';
import { placeName, weatherCityFor } from './weather';

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

describe('weatherCityFor', () => {
  it.each([
    ['weather', ''],
    ['Forecast', ''],
    ['weather raleigh', 'raleigh'],
    ['weather raleigh nc', 'raleigh nc'],
    ['weather in raleigh, nc', 'raleigh, nc'],
    ['forecast for stockholm sweden', 'stockholm sweden'],
    ['weather at leeds uk', 'leeds uk'],
    ['stockholm sweden weather', 'stockholm sweden'],
    ['Raleigh NC forecast', 'Raleigh NC'],
  ])('%s asks about "%s"', (text, city) => {
    expect(weatherCityFor(text)).toBe(city);
  });

  it.each(['weathervane', 'raleigh', 'forecasting tools', ''])(
    '%s is not a weather question',
    (text) => {
      expect(weatherCityFor(text)).toBeNull();
    }
  );
});
