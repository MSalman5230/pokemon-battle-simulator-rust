#!/usr/bin/env node
// Capture one-ply search replies from Poke-RL's actual public-belief API.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {Game, PublicState, permutations} = require('./worker.cjs');
const {lookahead} = require('./lookahead.cjs');
const corpus = JSON.parse(readFileSync(new URL('../benchmarks/verified-bss-corpus.json', import.meta.url)));
const game = new Game(corpus.teams.map(row => row.team), [4, 33, 2, 4], 200, true);
const previewId = ids => 14 + permutations.findIndex(p => p.join(',') === ids.join(','));
const state = game.step([previewId([3, 0, 1]), previewId([0, 1, 2])]);
const observation = structuredClone(state.views[0]);
const teams = structuredClone(game.teams);
const {turn, format} = observation;
const sides = game.battle.sides.map(side => {
    const pokemon = side.pokemon.map(mon => ({
        identity: mon.species.baseSpecies, set: {...mon.set}, stats: {hp: mon.maxhp, ...mon.storedStats},
        hp: mon.hp, maxhp: mon.maxhp, types: [...mon.types], status: mon.status,
        ability: mon.ability, base_ability: mon.baseAbility, item: mon.item, times_attacked: 0,
        moves: mon.moveSlots.map(m => ({id: m.id, pp: m.pp})),
    }));
    return {pokemon, boosts: {}, volatiles: [], observation: {}, fresh: true,
        last_move: null, mega_used: false, total_fainted: 0, conditions: {}};
});
function world(seed, rollout_seeds) {
    return {teams: structuredClone(teams), input: {format, turn, kind: 'turn', sides: structuredClone(sides),
        field: {weather: '', weather_turns: 1, terrain: '', terrain_turns: 1, trickroom: 0, extra: {}},
        wishes: {}, future_moves: {}}, seed, rollout_seeds};
}
const pivot = world([4, 30, 512, 100], [[1, 2, 3, 8], [1, 2, 3, 9]]);
const confusion = world([90, 400, 42, 6], [[3, 4, 5, 1], [3, 4, 5, 2]]);
confusion.input.sides[0].volatiles.push({id: 'confusion', elapsed: 2});
const fragile = world([11, 3, 5, 18], [[5, 6, 1, 12]]);
fragile.input.sides[1].pokemon[0].hp = 1;
const injuredObservation = structuredClone(observation);
injuredObservation.opponent.find(m => m.active).hp = 0.01;
const shield = world([12, 34, 56, 78], [[7, 2, 3, 4]]);
const next = shield.input.sides[0].pokemon[1];
shield.input.sides[0].volatiles.push({id: 'substitute', source: next.identity});
// A named bench source has no current field slot in singles. It must not
// redirect the residual damage to the currently active Pokémon.
shield.input.sides[1].volatiles.push({id: 'leechseed', source_side: 'me', source: next.identity});
const hazardOrder = world([4, 30, 512, 100], [[1, 2, 3, 8]]);
hazardOrder.input.sides[0].conditions = {toxicspikes: 1, stealthrock: 1, spikes: 2};
hazardOrder.input.sides[0].pokemon[1].hp = 1;
const bound = world([34, 12, 59, 63], [[9, 2, 3, 4]]);
bound.input.sides[0].volatiles.push({id:'partiallytrapped', source_side:'opp', move:'infestation', elapsed:2});
const revival = world([12, 32, 54, 79], [[5, 6, 7, 8]]);
for (const [side, [id, name, pp]] of [['revivalblessing', 'Revival Blessing', 1], ['recover', 'Recover', 8]].entries()) {
    const mon = revival.input.sides[side].pokemon[0];
    mon.moves[0] = {id, pp};
    mon.set.moves[0] = name;
    revival.teams[side].find(s => s.name === mon.set.name).moves[0] = name;
}
revival.input.sides[0].pokemon[1].hp = 0;
revival.input.sides[0].total_fainted = 1;
const delayed = world([23, 91, 42, 16], [[3, 11, 17, 21]]);
delayed.input.sides[0].pokemon[0].hp -= 12;
delayed.input.wishes.me = {source: delayed.input.sides[0].pokemon[0].identity, turns: 1};
delayed.input.future_moves.opp = {sourceSide: 'me', source: delayed.input.sides[0].pokemon[2].identity,
    move: 'Future Sight', turns: 1};
const savedTypes = world([28, 3, 41, 35], [[4, 9, 4, 10]]);
const typedActive = savedTypes.input.sides[0].pokemon[0];
typedActive.types = ['Water', 'Flying', '???'];
typedActive.ability = 'protean';
typedActive.type_ability_used = 'Protean';
const unusedProtean = structuredClone(savedTypes);
delete unusedProtean.input.sides[0].pokemon[0].type_ability_used;
const transformed = world([31, 6, 12, 27], [[9, 3, 6, 28]]);
transformed.input.sides[0].pokemon[0].transformed = {
    form: 'Jolteon', types: ['Electric', 'Flying'], ability: 'Volt Absorb',
    stats: {atk: 108, def: 80, spa: 160, spd: 115, spe: 177},
    moves: [{name: 'Thunderbolt', pp: 5}, {name: 'Quick Attack', pp: 5},
        {name: 'Protect', pp: 5}, {name: 'Volt Switch', pp: 5}],
    pp_used: {'Thunderbolt': 3, 'Protect': 5}, copied_effects: {focusenergy: {}},
};
const doubleSend = world([5, 12, 24, 36], [[8, 7, 6, 5]]);
doubleSend.input.kind = 'send';
for (const side of doubleSend.input.sides) {
    side.pokemon[0].hp = 0;
    side.total_fainted = 1;
    side.fainted_this_turn = side.pokemon[0].set.name;
    side.replacement = {kind:'faint'};
}
const doubleSendObservation = structuredClone(observation);
doubleSendObservation.own.find(m => m.active).hp = 0;
doubleSendObservation.opponent.find(m => m.active).hp = 0;
const soloSend = structuredClone(doubleSend);
soloSend.input.sides[1] = structuredClone(sides[1]);
soloSend.input.sides[1].replacement = {kind:'none'};
const perish = world([51, 11, 19, 24], [[8, 4, 12, 16]]);
perish.input.sides[0].volatiles.push({id:'perish1',remaining:1});
const room = world([35, 18, 6, 42], [[2, 5, 4, 9]]);
room.input.field.extra.magicroom = {remaining:2};
const cases = [
    {name: 'paired pivots and basic attacks', input: {observation, worlds: [pivot, confusion], candidates: [3, 1, 8],
        replies: 2, reply_probabilities: {'move:earthquake': 1}}},
    {name: 'unknown action is reported per world', input: {observation, worlds: [pivot], candidates: [99],
        replies: 1, reply_probabilities: {}}},
    {name: 'replace a fainted opponent using its healthiest legal bench', input: {observation: injuredObservation,
        worlds: [fragile], candidates: [0], replies: 1, reply_probabilities: {'move:earthquake': 1}}},
    {name: 'restore Substitute from its owner and Leech Seed from a named bench source', input: {observation,
        worlds: [shield], candidates: [1], replies: 1, reply_probabilities: {'move:earthquake': 1}}},
    {name: 'preserve insertion order for hazards on replacement', input: {observation,
        worlds: [hazardOrder], candidates: [8], replies: 1, reply_probabilities: {'move:earthquake': 1}}},
    {name: 'restore a binding move from a currently active source', input: {observation,
        worlds: [bound], candidates: [1], replies: 1, reply_probabilities: {'move:earthquake': 1}}},
    {name: 'continue a Revival Blessing request when every legal target has zero HP', input: {observation,
        worlds: [revival], candidates: [0], replies: 1, reply_probabilities: {'move:recover': 1}}},
    {name: 'restore delayed healing and an attack from a named bench source', input: {observation,
        worlds: [delayed], candidates: [1], replies: 1, reply_probabilities: {'move:protect': 1}}},
    {name: 'restore dual and typeless typing separately from the used Protean flag', input: {observation,
        worlds: [savedTypes, unusedProtean], candidates: [1], replies: 1, reply_probabilities: {'move:earthquake': 1}}},
    {name: 'restore Transform with copied stats, critical boost, spent PP and switch reset', input: {observation,
        worlds: [transformed], candidates: [0, 8], replies: 1, reply_probabilities: {'move:earthquake': 1}}},
    {name: 'sample simultaneous forced replacements from a public send request', input: {observation:doubleSendObservation,
        worlds:[doubleSend], candidates:[8,9], replies:1, reply_probabilities:{'switch:1':1}}},
    {name: 'a one-sided replacement has no simultaneous opponent reply', input: {observation:doubleSendObservation,
        worlds:[soloSend], candidates:[8], replies:1, reply_probabilities:{}}},
    {name: 'restore a Perish Song count before its final residual', input: {observation,
        worlds:[perish], candidates:[1], replies:1, reply_probabilities:{'move:earthquake':1}}},
    {name: 'restore Magic Room without re-running its entry effects', input: {observation,
        worlds:[room], candidates:[1], replies:1, reply_probabilities:{'move:earthquake':1}}},
];
for (const c of cases) c.expected = lookahead(c.input, Game, PublicState);
assert(cases[0].expected.errors.length === 0, JSON.stringify(cases[0].expected.errors));
assert(cases[0].expected.rows.some(row => row.continuations > 0), 'fixture must exercise pivot continuation');
assert(cases[0].expected.rows.some(row => row.assumptions.length > 0), 'fixture must report reconstruction assumptions');
assert(cases[2].expected.rows.some(row => row.continuations > 0 && row.observation.faints[1].length > 0),
    'fixture must exercise replacement after a faint');
assert(cases[3].expected.rows.some(row => row.assumptions.some(a => a.fact === 'substitute HP')));
assert(cases[4].expected.rows.some(row => row.observation.faints[0].length > 0));
assert(cases[5].expected.rows.some(row => row.assumptions.some(a => a.fact === 'partial trap duration')));
assert(cases[6].expected.rows.some(row => row.continuations === 1), JSON.stringify(cases[6].expected.errors));
assert(cases[7].expected.rows.length === 1 && !cases[7].expected.errors.length,
    JSON.stringify(cases[7].expected.errors));
assert(cases[8].expected.rows.length === 2 && !cases[8].expected.errors.length,
    JSON.stringify(cases[8].expected.errors));
assert.notDeepEqual(...cases[8].expected.rows.map(row => row.observation));
assert(cases[9].expected.rows.length === 2 && !cases[9].expected.errors.length,
    JSON.stringify(cases[9].expected.errors));
const [copied, reverted] = cases[9].expected.rows.map(row => row.observation.own[3]);
assert(copied.species === 'Jolteon' && copied.moves[0].pp === 1 && copied.volatile_ids.includes('focusenergy'));
assert(reverted.species === 'Swampert' && reverted.moves[0].id === 'earthquake' && reverted.moves[0].pp === 12);
assert(cases[10].expected.rows.length === 2 && !cases[10].expected.errors.length, JSON.stringify(cases[10].expected.errors));
assert.deepEqual(cases[11].expected.errors, [{world:0,error:'Opponent has no simultaneous reply'}]);
assert(cases[12].expected.rows.some(row=>row.continuations>0));
assert(cases[13].expected.rows.length === 1 && !cases[13].expected.errors.length, JSON.stringify(cases[13].expected.errors));
writeFileSync(new URL('../tests/fixtures/lookahead.json', import.meta.url),
    `{"revision":${JSON.stringify(revision)},"cases":[\n${cases.map(c => JSON.stringify(c)).join(',\n')}\n]}\n`);
