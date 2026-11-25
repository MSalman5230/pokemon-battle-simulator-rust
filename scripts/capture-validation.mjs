#!/usr/bin/env node
// Oracle verdicts for ordinary BSS and VGC sets and roster construction.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {TeamValidator} = require('pokemon-showdown');
const base = {species: 'Venusaur', moves: ['Sludge Bomb', 'Sleep Powder', 'Energy Ball', 'Protect'],
    ability: 'Chlorophyll', nature: 'Modest', evs: {hp: 32, spa: 32, spd: 2}};
const eternal = {species: 'Floette-Eternal', moves: ['Light of Ruin', 'Moonblast', 'Trick', 'Giga Drain'], ability: 'Flower Veil'};
const sets = [
    ['legal set', {}],
    ['unknown species', {species: 'Not a Pokémon'}],
    ['past item', {item: 'Ability Shield'}],
    ['mythical species', {species: 'Mew'}],
    ['unknown item', {item: 'Some fake item'}],
    ['unknown nature', {nature: 'Not a Nature'}],
    ['wrong ability', {ability: 'Adaptability'}],
    ['missing ability', {ability: ''}],
    ['empty moves', {moves: []}],
    ['duplicate move', {moves: ['Tackle', 'Tackle']}],
    ['unlearnable move', {moves: ['Dragon Ascent']}],
    ['unknown move', {moves: ['example'], ability: 'Adaptability'}],
    ['stat point bounds', {evs: {hp: 40, atk: -1, spa: 32, spd: 9}}],
    ['perfect IVs', {ivs: {atk: 0}}],
    ['level bounds', {level: 1000}],
    ['species nickname', {name: 'Bulbasaur'}],
    ['starting form of Mega', {species: 'Venusaur-Mega', item: 'Venusaurite', ability: 'Thick Fat'}],
    ['modern event can use mints', {...eternal, level: 50, nature: 'Timid'}],
    ['shiny-locked event', {...eternal, level: 50, shiny: true}],
    ['underlevel event', {...eternal, level: 70}],
];
const corpus = JSON.parse(readFileSync(new URL('../benchmarks/verified-bss-corpus.json', import.meta.url))).teams;
const team = corpus[0].team;
const teams = [
    ['legal roster', team],
    ['too few members', team.slice(0, 5)],
    ['too many members', [...team, base]],
    ['duplicate species', [...team.slice(0, 5), {...team[0], item: ''}]],
    ['duplicate items', team.map((set, i) => i < 2 ? {...set, item: 'Leftovers'} : set)],
    ['duplicate nicknames', team.map((set, i) => i < 2 ? {...set, name: 'Twice'} : set)],
];
const formats = ['gen9championsbssregmc', 'gen9championsvgc2026regmc'];
const fixtures = formats.flatMap(format => {
    const validator = new TeamValidator(format);
    return [
        ...sets.map(([name, changes]) => ({name, format, set: {...base, ...changes},
            expected: validator.validateSet(structuredClone({...base, ...changes}))})),
        ...teams.map(([name, team]) => ({name, format, team,
            expected: validator.validateTeam(structuredClone(team))})),
    ];
});
writeFileSync(new URL('../tests/fixtures/validation.json', import.meta.url),
    JSON.stringify({revision, fixtures}, null, 2) + '\n');
