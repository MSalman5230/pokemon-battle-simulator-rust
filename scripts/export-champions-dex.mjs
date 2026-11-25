#!/usr/bin/env node
// Export static data only. Never serialize or execute JS battle handlers in Rust.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {speciesIncludingCosmetics} from './showdown-species.mjs';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {Battle, Dex} = require('pokemon-showdown');
const root = new URL('../', import.meta.url);
const sorted = entries => Object.fromEntries(entries.sort(([a], [b]) => a.localeCompare(b, 'en')));
const table = (entries, project) => sorted(entries.map(e => [e.id, project(e)]));
const props = (e, keys) => Object.fromEntries(keys.map(key => [key, e[key]]));
const handlers = e => Object.keys(e).filter(k =>
    typeof e[k] === 'function' || /^on[A-Z]/.test(k)).sort();

// Only opt a move into the generic pipeline when every battle-relevant field
// is covered. An omitted handler or unrecognized nested effect is not inert.
const genericMoveFields = new Set([
    'num', 'name', 'accuracy', 'basePower', 'category', 'pp', 'priority', 'flags',
    'target', 'type', 'contestType', 'isNonstandard', 'zMove', 'maxMove', 'tags',
    'noPPBoosts', 'critRatio', 'willCrit', 'alwaysHit', 'heal', 'boosts',
    'status', 'ignoreImmunity', 'self', 'recoil', 'drain', 'thawsTarget',
    'secondary', 'secondaries', 'multihit', 'multiaccuracy', 'selfBoost',
    'ignoreDefensive', 'ignoreEvasion', 'tracksTarget',
    'overrideDefensiveStat',
]);
const genericTarget = new Set(['self', 'normal', 'any', 'adjacentFoe', 'allAdjacent', 'allAdjacentFoes', 'randomNormal']);
const supportedStatus = new Set(['brn', 'par', 'frz', 'slp', 'psn', 'tox']);
function hasOnly(object, keys) {
    return Object.keys(object).every(key => keys.includes(key));
}
function runsOnGenericPipeline(raw, move) {
    if (!raw || Object.keys(raw).some(k => !genericMoveFields.has(k)) ||
        !genericTarget.has(move.target) || raw.flags?.futuremove ||
        typeof raw.ignoreImmunity === 'object' ||
        (raw.overrideDefensiveStat && raw.overrideDefensiveStat !== 'def') ||
        (raw.status && !supportedStatus.has(raw.status))) return false;
    if (move.multihit && !['normal', 'any', 'adjacentFoe'].includes(move.target)) return false;
    if (raw.self && !(raw.self.boosts && hasOnly(raw.self, ['boosts', 'chance']) ||
            ['mustrecharge', 'lockedmove'].includes(raw.self.volatileStatus) && hasOnly(raw.self, ['volatileStatus']))) return false;
    if (raw.selfBoost && !(raw.selfBoost.boosts && hasOnly(raw.selfBoost, ['boosts']))) return false;
    for (const effect of move.secondaries ?? []) {
        if (!hasOnly(effect, ['chance', 'status', 'volatileStatus', 'boosts', 'self']) ||
            (effect.self && (!effect.self.boosts || !hasOnly(effect.self, ['boosts']))) ||
            (effect.status && !supportedStatus.has(effect.status)) ||
            (effect.volatileStatus && !['flinch', 'confusion'].includes(effect.volatileStatus))) return false;
    }
    if (move.category === 'Status') return !!(move.boosts || move.status || move.heal);
    return move.basePower > 0 && !move.status && !move.heal;
}

function flingData(item) {
    if (!item.fling) return undefined;
    const {basePower, status, volatileStatus, effect} = item.fling;
    const special = effect ? {mentalherb:'mentalHerb', whiteherb:'whiteHerb'}[item.id] : undefined;
    assert(!effect || special, `Unmapped Fling effect for ${item.id}`);
    return {basePower, status, volatileStatus, effect:special};
}

const data = mod => {
    const dex = Dex.mod(mod);
    return {
        species: table(speciesIncludingCosmetics(dex), e => {
            const packedSlots = Dex.species.get(e.name).abilities;
            return {
            ...props(e, ['name', 'baseSpecies', 'types', 'baseStats', 'weighthg', 'gender', 'abilities',
                'isNonstandard', 'maxHP', 'requiredItem']),
            hasCondition: dex.conditions.getByID(e.id).exists,
            unreleasedHidden: !!e.unreleasedHidden,
            packedAbilitySlots: JSON.stringify(packedSlots) === JSON.stringify(e.abilities) ? undefined : packedSlots,
            };
        }),
        moves: sorted(dex.moves.all().map(e => {
            const move = props(e, ['name', 'type', 'category', 'target', 'basePower', 'priority',
                'pp', 'noPPBoosts', 'accuracy', 'multihit', 'multiaccuracy', 'critRatio', 'flags', 'isNonstandard', 'heal', 'status', 'ignoreImmunity', 'drain', 'recoil', 'thawsTarget', 'willCrit', 'alwaysHit']);
            const key = e.id === 'hiddenpower' ? Dex.toID(e.name) : e.id;
            return [key, {...move, ignoreDefensive: e.ignoreDefensive || undefined,
                targetsDefense: e.overrideDefensiveStat === 'def' || undefined,
                ohko: !!e.ohko || undefined,
                ignoreEvasion: e.ignoreEvasion || undefined, tracksTarget: e.tracksTarget || undefined,
                cantUseTwice: !!e.flags?.cantusetwice || undefined,
                hasSheerForceBoost: e.hasSheerForceBoost || undefined,
                generic: runsOnGenericPipeline(dex.data.Moves[e.id], e), handlers: handlers(e), secondary: !!e.secondary || !!e.secondaries?.length,
                self: !!e.self, condition: !!e.condition, boosts: Object.entries(e.boosts ?? {}),
                selfBoosts: Object.entries(e.self?.boosts ?? {}), selfChance: e.self?.chance,
                afterHitBoosts: e.selfBoost?.boosts ? Object.entries(e.selfBoost.boosts) : undefined,
                selfRecharge: e.self?.volatileStatus === 'mustrecharge' || undefined,
                selfRampage: e.self?.volatileStatus === 'lockedmove' || undefined,
                secondaries: (e.secondaries ?? []).map(s => ({chance: s.chance, status: s.status,
                    volatileStatus: s.volatileStatus, boosts: Object.entries(s.boosts ?? {}),
                    selfBoosts: s.self?.boosts && Object.entries(s.self.boosts)}))}];
        })),
        abilities: table(dex.abilities.all(), e =>
            ({...props(e, ['name', 'isNonstandard']), handlers: handlers(e),
                noTrace: !!e.flags?.notrace || undefined, cannotSuppress: !!e.flags?.cantsuppress || undefined,
                breakable: !!e.flags?.breakable || undefined,
                noReceiver: !!e.flags?.noreceiver || undefined,
                noTransform: !!e.flags?.notransform || undefined,
                noEntrain: !!e.flags?.noentrain || undefined,
                noSkillSwap: !!e.flags?.failskillswap || undefined})),
        items: table(dex.items.all(), e =>
            ({...props(e, ['name', 'isNonstandard', 'megaStone']),
                isBerry: e.isBerry || undefined, ignoreKlutz: e.ignoreKlutz || undefined, fling: flingData(e),
                handlers: handlers(e)})),
        natures: table(dex.natures.all(), e => props(e, ['plus', 'minus'])),
        types: table(dex.types.all(), e => props(e, ['damageTaken'])),
        aliases: sorted([...Dex.loadAliases()]),
    };
};
const main = data('champions');
const mb = data('championsregmb');
// Store different records only. Some Reg M-B moves change PP, not just legality.
const regmb = Object.fromEntries(Object.entries(mb).map(([key, records]) => {
    assert.deepEqual(Object.keys(records), Object.keys(main[key]));
    return [key, Object.fromEntries(Object.entries(records)
        .filter(([id, value]) => JSON.stringify(value) !== JSON.stringify(main[key][id])))];
}).filter(([, records]) => Object.keys(records).length));

const formats = {};
for (const f of Dex.formats.all().filter(f => /^\[Gen 9 Champions\]/.test(f.name))) {
    const r = Dex.formats.getRuleTable(f);
    const battle = new Battle({formatid: f.id, seed: '1,2,3,4'});
    for (const player of ['p1', 'p2']) {
        battle.setPlayer(player, {team: [
            {species: 'Magikarp', moves: ['Splash'], gender: 'M'},
            {species: 'Milotic', moves: ['Recover'], gender: 'F'},
        ]});
    }
    const ruleLog = battle.log.filter(line => line.startsWith('|rule|'));
    battle.destroy();
    formats[f.id] = {
        name: f.name, mod: f.mod, gameType: f.gameType, generator: f.team || null,
        rules: [...r.keys()], minTeamSize: r.minTeamSize, maxTeamSize: r.maxTeamSize,
        maxMoveCount: r.maxMoveCount, pickedTeamSize: r.pickedTeamSize, defaultLevel: r.defaultLevel,
        bestOf: r.valueRules.has('bestof') ? Number(r.valueRules.get('bestof')) : undefined,
        adjustLevel: r.adjustLevel, adjustLevelDown: r.adjustLevelDown, maxLevel: r.maxLevel,
        unboundedTruncation: !!f.battle?.trunc, reportExactHp: !!f.debug, ruleLog,
    };
}
function recordLines(obj) {
    return '{\n' + Object.entries(obj).map(([key, value]) => JSON.stringify(key) + ': ' +
        (typeof value === 'object' && !Array.isArray(value) ?
            (key === 'regmb' ? recordLines(value) : '{\n' + Object.entries(value)
                .map(([id, row]) => JSON.stringify(id) + ': ' + JSON.stringify(row)).join(',\n') + '\n}') :
            JSON.stringify(value))).join(',\n') + '\n}';
}
writeFileSync(new URL('data/champions.json', root), recordLines({revision, formats, ...main, regmb}) + '\n');
