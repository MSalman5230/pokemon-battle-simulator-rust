#!/usr/bin/env node
// Rebuild fixtures with the exact Showdown dependency used by Poke-RL.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const {Battle, BattleStream, PRNG, TeamValidator} = require('pokemon-showdown');
const {extractChannelMessages} = require('pokemon-showdown/dist/sim/battle');
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const save = (file, data) => writeFileSync(new URL('../tests/fixtures/' + file, import.meta.url),
    JSON.stringify({revision, ...data}, null, 2) + '\n');

const seeds = ['0,0,0,0', '1,2,3,4', '65535,65535,65535,65535', 'gen5,ffff0a09abcdffff',
    'sodium,a', 'sodium,131415161718191a1b1c1d1e1f202122',
    'sodium,ffffff00000000000000000a0000000000000000000000000000000000000000'];
const rng = seeds.map(seed => {
    const prng = new PRNG(seed);
    const words = Array.from({length: 64}, () => prng.rng.next());
    const bounded = [0, 1, 24, 16, 100, 2, 65536, 4294967295].map(upper => [upper, prng.random(upper)]);
    return {seed, words, bounded, after: prng.getSeed()};
});

const fallback = [90, 10, 10, 70, 40, 90, 40, 10];
const sorting = [fallback, Array(20).fill(50), fallback.concat(Array(18).fill(300))].map((speeds, n) => {
    const seed = seeds[n];
    const battle = new Battle({formatid: 'gen9championsbssregmc', seed});
    const nodes = speeds.map((speed, i) => ({speed, i, priority: [0, -0.1, 0.1][(i + n) % 3]}));
    if (n > 0) nodes.forEach(p => p.priority = 0);
    const input = structuredClone(nodes);
    battle.speedSort(nodes);
    return {seed, input, output: nodes.map(x => x.i), after: battle.prng.getSeed()};
});

const battles = ['bssregmc', 'bssregmb', 'randombattle', 'customgame']
    .map(name => new Battle({formatid: 'gen9champions' + name, seed: seeds[1]}));
const stat = battles.flatMap((battle, mode) => ['atk', 'spe', 'hp'].flatMap(name =>
    [0, 1, 2, 32, 255].map((points, i) => {
        const base = [50, 100, 95, 250, 255][i];
        const level = [1, 42, 50, 100, 9999][i];
        const nature = ['AdaMant', 'timid', 'serious', 'brave', 'hasty'][i];
        const set = {evs: {[name]: points}, level, nature};
        return {mode, name, points, base, level, nature, result: battle.statModify({[name]: base}, set, name)};
    })));
const normal = battles[0];
const healthMon = {species: 'Magikarp', moves: ['Splash'], gender: 'F'};
normal.setPlayer('p1', {team: [healthMon]});
const mon = normal.sides[0].pokemon[0];
const health = [
    [0, 201, ''], [1, 333, ''], [40, 200, ''], [41, 204, ''], [100, 200, ''],
    [100, 199, 'brn'], [200, 200, 'tox'], [51, 103, ''],
].flatMap(([hp, maxhp, status]) => [false, true].map(exact => {
    Object.assign(mon, {hp, maxhp, status});
    normal.reportExactHP = exact;
    const {secret, shared} = mon.getHealth();
    return {hp, maxhp, status, exact, secret, shared};
}));
save('mechanics.json', {rng, sorting, stat, health});

const named = (species, moves, gender = 'F') => ({
    species, moves, gender, name: species, level: 50, nature: 'Hardy', ability: '',
});
const rosters = [
    [named('Flareon', ['Tackle', 'Quick Attack', 'Protect']), named('Vaporeon', ['Water Gun', 'Surf']),
        named('Jolteon', ['Thunder Shock']), named('Glaceon', ['Ice Shard'])],
    [named('Umbreon', ['Tackle']), named('Espeon', ['Swift', 'Protect']), named('Leafeon', ['Tackle', 'Protect']),
        named('Sylveon', ['Quick Attack'])],
];
const replacementRosters = [
    [{...named('Vaporeon', ['Surf']), evs: {spa: 32}, nature: 'Modest'}, named('Jolteon', ['Protect']),
        named('Glaceon', ['Ice Shard'])],
    [named('Camerupt', ['Tackle']), named('Rhydon', ['Tackle']), named('Flareon', ['Quick Attack'])],
];
const competitiveSets = [
    ['Infernape', 'Blaze', ['Close Combat', 'Protect', 'Swords Dance', 'Earthquake']],
    ['Empoleon', 'Torrent', ['Surf', 'Aqua Jet', 'Swords Dance', 'Protect']],
    ['Meganium', 'Overgrow', ['Swords Dance', 'Earthquake', 'Leaf Blade', 'Protect']],
    ['Swampert', 'Torrent', ['Earthquake', 'Surf', 'Protect', 'Dive']],
    ['Sceptile', 'Overgrow', ['Leaf Blade', 'Earthquake', 'Swords Dance', 'Protect']],
    ['Blaziken', 'Blaze', ['Close Combat', 'Protect', 'Swords Dance', 'Earthquake']],
].map(([species, ability, moves]) => ({
    ...named(species, moves), ability, nature: 'Adamant', evs: {hp: 2, atk: 32, spe: 32},
}));
const competitiveRosters = [competitiveSets, [...competitiveSets].reverse()];
const statusRosters = [
    [named('Vaporeon', ['Thunder Wave', 'Recover', 'Surf', 'Ice Beam']),
        named('Flareon', ['Will-O-Wisp', 'Tackle']), named('Jolteon', ['Nuzzle'])],
    [named('Umbreon', ['Toxic', 'Tackle', 'Protect', 'Hypnosis']),
        {...named('Glaceon', ['Tackle', 'Ice Beam', 'Recover', 'Protect']), item: 'Leftovers'}, named('Espeon', ['Swift'])],
];
const sleepFreezeRosters = [
    [named('Jolteon', ['Ice Beam', 'Recover']), named('Flareon', ['Flamethrower'])],
    [named('Umbreon', ['Tackle', 'Hypnosis', 'Recover']), named('Vaporeon', ['Recover'])],
];
const itemRosters = [
    [{...named('Flareon', ['Tackle']), item: 'Life Orb'},
        {...named('Vaporeon', ['Surf']), item: 'Mystic Water'},
        {...named('Jolteon', ['Earthquake', 'Water Gun', 'Toxic', 'Recover']), item: 'Sitrus Berry'}],
    [{...named('Garchomp', ['Protect', 'Tackle']), ability: 'Rough Skin', item: 'Rocky Helmet'},
        {...named('Camerupt', ['Protect', 'Tackle']), item: 'Focus Sash'},
        {...named('Gholdengo', ['Shadow Ball', 'Recover']), ability: 'Good as Gold', item: 'Air Balloon'}],
];
for (const [suffix, play, players = rosters, label = suffix, seed = seeds[1]] of [
    ['bssregmc', [['team 132', 'team 231'], ['move 2', 'move 1'], ['switch 2', 'move 1']]],
    ['vgc2026regmc', [['team 1234', 'team 2134'], ['move 1 2, move 2', 'move 2, move 1 1'],
        ['move 2 2, switch 4', 'move 1, move 1 1']]],
    ['vgc2026regmc', [['team 123', 'team 123'], ['move 1, move 1', 'move 1 1, move 1 1'],
        [null, 'pass, switch 3'], ['move 1, move 1', 'move 1 1'],
        ['move 1, move 1', 'move 1 1']], replacementRosters, 'vgc-replacements'],
    ['bssregmc', [['team 123', 'team 365'], ['move 3', 'move 2'], ['move 1', 'move 3'],
        ['move 1', 'move 2'], [null, 'switch 2'], ['move 1', 'move 2']],
        competitiveRosters, 'bss-competitive'],
    ['bssregmc', [['team 123', 'team 123'], ['move 1', 'move 1'], ['move 2', 'move 2'],
        ['switch 2', 'move 2'], ['move 1', 'switch 2'], ['move 2', 'move 1'],
        ['switch 2', 'move 3'], ['move 4', 'move 2'], ['move 2', 'move 2']],
        statusRosters, 'bss-status'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 2'],
        ...Array(3).fill(['move 2', 'move 2']), ...Array(3).fill(['move 2', 'move 1'])],
        sleepFreezeRosters, 'bss-sleep-freeze', '1,2,3,22'],
    ['bssregmc', [['team 123', 'team 123'], ['move 1', 'move 2'],
        ['switch 2', 'switch 2'], ['move 1', 'move 2'], ['move 1', 'move 2'],
        [null, 'switch 3'], ['switch 3', 'move 2'], ['move 1', 'move 1'],
        ['move 3', 'move 2'], ['move 2', 'move 2'], ['move 4', 'move 1']], itemRosters, 'bss-items'],
]) {
    const formatid = 'gen9champions' + suffix;
    if (players === competitiveRosters) {
        const validator = new TeamValidator(formatid);
        for (const team of players) assert.equal(validator.validateTeam(structuredClone(team)), null);
    }
    const inputs = [
        '>start ' + JSON.stringify({formatid, seed, strictChoices: true}),
        ...players.map((team, i) => '>player p' + (i + 1) + ' ' + JSON.stringify({name: 'player' + i, team})),
    ];
    const stream = new BattleStream({noCatch: true});
    let logCursor = 0;
    const checkpoints = [];
    async function checkpoint(lines) {
        for (const line of lines) await stream.write(line);
        const battle = stream.battle;
        const newLog = battle.log.slice(logCursor).filter(l => !l.startsWith('|t:|'));
        logCursor = battle.log.length;
        // Open Team Sheet prompts are UI, outside the headless engine.
        const channels = extractChannelMessages(newLog.join('\n'), [0, 1, 2]);
        checkpoints.push({
            lines,
            turn: battle.turn,
            seed: battle.prng.getSeed(),
            requestState: battle.requestState,
            channels: [0, 1, 2].map(id => channels[id].filter(l => !l.startsWith('|uhtml|otsrequest|') && !l.startsWith('|uhtmlchange|otsrequest|'))),
            requests: battle.sides.map(s => s.activeRequest),
            teams: battle.sides.map(s => s.pokemon.map(p => ({
                name: p.name, species: p.species.name, details: p.details, hp: p.hp, maxhp: p.maxhp, status: p.status,
                stats: p.storedStats, boosts: {...p.boosts}, ability: p.ability, item: p.item, active: p.isActive,
                moves: p.moveSlots.map(m => ({id: m.id, pp: m.pp, maxpp: m.maxpp})),
            }))),
        });
    }
    await checkpoint(inputs);
    for (const choices of play) await checkpoint(choices.flatMap((c, i) => c ? ['>p' + (i + 1) + ' ' + c] : []));
    await stream.writeEnd();
    save(label + '.json', {formatid, checkpoints});
}
