#!/usr/bin/env node
// Fixed hypothetical queries, run through Poke-RL's own pinned calculator.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {calculateMany} = require('./tactics.cjs');

const mon = (species, ability = '', item = '', state = {}, set = {}) => ({
    set: {species, ability, item, nature: 'Hardy', moves: ['Protect'], gender: 'F', ...set}, ...state,
});
const fire = mon('Charizard', 'Blaze');
const water = mon('Vaporeon', 'Water Absorb');
const normal = mon('Umbreon', 'Inner Focus');
const electric = mon('Jolteon', 'Volt Absorb');
const queries = [];
function add(name, move, actor, target, env) { queries.push({name, input: {move, actor, target, ...env}}); }
add('basic full health', 'flamethrower', fire, normal);
add('different move with the same prepared state', 'airslash', fire, normal);
add('no damage from a status move', 'protect', fire, normal);
add('blaze applies at one third HP', 'flamethrower', {...fire, hp: .3}, normal);
add('water absorb replaces all rolls with zero', 'watergun', normal, water);
add('type immunity', 'earthquake', normal, fire);
add('fixed damage', 'seismictoss', water, normal);
add('frozen target and fire attack', 'flamethrower', fire, {...normal, status: 'frz'});
add('focus flags use public item only', 'quickattack', electric, {...normal, item: 'Focus Sash'});
add('damage still rolls at Focus Sash', 'hyperbeam', normal, {...normal, item: 'Focus Sash', hp: .25});
add('scald in rain', 'scald', water, normal, {weather: 'raindance'});
add('weather boosts base power without running ModifyType', 'weatherball', electric, fire, {weather: 'raindance'});
add('conversion abilities do not run their ModifyType hook', 'tackle', mon('Sylveon', 'Pixilate'),
    mon('Dragapult', 'Infiltrator'));
add('tailwind and screens are side-specific', 'scald', water, normal,
    {conditions: [{tailwind: {}}, {lightscreen: {}}]});
add('choice item, burn, stat stages, and stats', 'quickattack',
    mon('Flareon', 'Guts', 'Choice Band', {status: 'brn', boosts: {atk: 2}, stats: {atk: 108}}),
    {...normal, boosts: {def: 1}, stats: {def: 180}});
add('paralysis and choice scarf speed', 'quickattack',
    mon('Jolteon', 'Volt Absorb', 'Choice Scarf', {status: 'par'}), normal);
add('guaranteed critical hit disregards unfavorable stages', 'wickedblow',
    {...normal, boosts: {atk: -2}}, {...water, boosts: {def: 2}});
add('storm accuracy in rain', 'thunder', electric, normal, {weather: 'raindance'});
add('sight accuracy uses fractional stage modifiers', 'thunder', {...electric, boosts: {accuracy: -1}}, normal);
add('psychic terrain boosts grounded attackers', 'psychic', electric, normal, {terrain: 'psychicterrain'});
add('grassy terrain weakens targeted Earthquake', 'earthquake', normal, electric, {terrain: 'grassyterrain'});
add('specified material survives without private game state', 'quickattack',
    {...normal, types: ['Steel']}, {...water, types: ['Ghost'], max_hp: 123});
add('rage fist uses hypothetical hit count', 'ragefist', {...normal, times_attacked: 3}, water);
add('eruption uses hypothetical remaining HP', 'eruption', {...fire, hp: .4, max_hp: 101}, normal);
add('cracked disguise takes one eighth of its full HP', 'shadowball', normal,
    mon('Mimikyu', 'Disguise', '', {hp: .6, max_hp: 124}));
add('flash fire volatile powers its next attack', 'flamethrower',
    mon('Ninetales', 'Flash Fire', '', {volatiles: {flashfire: {}}}), normal);
add('hazards do not affect direct damage', 'watergun', normal, {...electric, item: 'Shuca Berry'},
    {conditions: [{stealthrock: {}}, {stickyweb: {}}]});
add('supported Mega changes stats and can set weather', 'flamethrower',
    mon('Charizard', 'Blaze', 'Charizardite Y'), normal, {mega: true});
add('stance change is applied during ModifyMove', 'shadowball',
    mon('Aegislash', 'Stance Change'), normal);
add('Hustle directly modifies attack before Choice Band', 'tackle',
    mon('Durant', 'Hustle', 'Choice Band'), normal);
add('Zoom Lens sees no pending target action in isolated damage queries', 'rockslide',
    mon('Durant', 'Hustle', 'Zoom Lens'), normal);
add('fractional stat stage is applied before accuracy callbacks', 'rockslide',
    mon('Durant', 'Hustle', 'Zoom Lens', {boosts:{accuracy:-1}}), normal);
add('Charge volatile powers the electric attack', 'thundershock',
    mon('Bellibolt', 'Electromorphosis', '', {volatiles:{charge:{}}}), normal);
add('Klutz suppresses a held damage item without removing it', 'tackle',
    mon('Lopunny', 'Klutz', 'Choice Band'), normal);
add('Reversal at the lowest HP bracket', 'reversal',
    mon('Machamp', 'Guts', '', {hp:.02,max_hp:200}), normal);
add('a Flying holder of Iron Ball takes neutral Ground damage', 'earthquake',
    normal, mon('Zapdos', 'Pressure', 'Iron Ball'));
add('Psyshock uses special offense against Fur Coat and Defense boosts', 'psyshock',
    mon('Espeon', 'Synchronize', 'Choice Specs'), mon('Furfrou', 'Fur Coat', '', {boosts:{def:2}}));
add('Psystrike uses physical defense including snow and not SpD', 'psystrike',
    mon('Espeon', 'Synchronize'), mon('Glaceon', 'Snow Cloak', '', {boosts:{spd:2}}), {weather:'snowscape'});
add('Acrobatics doubles with no held item', 'acrobatics', mon('Hawlucha', 'Unburden'), water);
add('Klutz does not make a held item disappear for Acrobatics', 'acrobatics',
    mon('Lopunny', 'Klutz', 'Choice Band'), water);
add('Hard Press power scales with the defender HP', 'hardpress', normal, {...water, hp:.27});
add('Rising Voltage doubles for a grounded target', 'risingvoltage', electric, normal, {terrain:'electricterrain'});
add('Rising Voltage does not double against a Flying target', 'risingvoltage', electric, fire, {terrain:'electricterrain'});
add('Fluffy halves contact damage', 'tackle', normal, mon('Bewear', 'Fluffy'));
add('Fluffy doubles noncontact fire damage', 'flamethrower', fire, mon('Bewear', 'Fluffy'));
add('Fluffy’s two multipliers cancel for contact Fire moves', 'flamecharge', fire, mon('Bewear', 'Fluffy'));
add('Punk Rock strengthens the user’s sound attack', 'hypervoice', mon('Toxtricity', 'Punk Rock'), water);
add('Punk Rock softens incoming sound damage', 'hypervoice', water, mon('Toxtricity', 'Punk Rock'));

const expected = calculateMany(queries.map(q => q.input));
const data = queries.map((q, i) => ({...q, expected: expected[i]}));
writeFileSync(new URL('../tests/fixtures/tactics.json', import.meta.url),
    `{"revision":${JSON.stringify(revision)},"cases":[\n${data.map(x => JSON.stringify(x)).join(',\n')}\n]}\n`);
