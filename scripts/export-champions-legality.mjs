#!/usr/bin/env node
// Export rule and learnset data, not validator verdicts or executable handlers.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {speciesIncludingCosmetics} from './showdown-species.mjs';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {Dex, TeamValidator} = require('pokemon-showdown');

function byID(records) {
    return Object.fromEntries(records.sort(([a], [b]) => a.localeCompare(b, 'en')));
}

function project(dex, legacy = false) {
    const learnsets = new Map();
    const species = byID(speciesIncludingCosmetics(dex).map(s => {
        let parents;
        try {
            parents = dex.species.getFullLearnset(s.id).map(data => {
                const parent = data.species.id;
                if (!learnsets.has(parent)) {
                    learnsets.set(parent, {
                        moves: data.learnset,
                        ...(data.eventData && {events: data.eventData}),
                        ...(data.eventOnly && {eventOnly: true}),
                        ...(data.encounters && {encounters: data.encounters}),
                    });
                }
                return {
                    species: parent,
                    ...(dex.species.eggMovesOnly(data.species, s) && {eggOnly: true}),
                };
            });
        } catch (error) {
            // This is also an explicit missing-data error in checkCanLearn.
            // Preserve that state, rather than treating it as an empty pool.
            if (!String(error).startsWith('Error: Species with no learnset data: ')) throw error;
            parents = null;
        }
        const go = !legacy && dex.species.getPokemonGoData(s.id);
        return [s.id, {
            num: s.num, gen: s.gen, tags: s.tags, tier: s.tier, doublesTier: s.doublesTier,
            forme: s.forme, baseForme: s.baseForme, battleOnly: s.battleOnly,
            changesFrom: s.changesFrom, requiredAbility: s.requiredAbility, requiredItems: s.requiredItems,
            requiredMove: s.requiredMove, evoLevel: s.evoLevel, canHatch: s.canHatch, mother: s.mother,
            prevo: s.prevo, nfe: s.nfe, eggGroups: s.eggGroups, parents,
            ...(go && {pokemonGo: go}),
            ...(legacy && {natDexTier: s.natDexTier, gender: s.gender, evoRegion: s.evoRegion}),
        }];
    }));
    return {species, learnsets: byID([...learnsets])};
}

const championsDex = Dex.mod('champions');
const champions = project(championsDex);
const mb = project(Dex.mod('championsregmb'));
const regmb = Object.fromEntries(Object.entries(mb).map(([key, rows]) => [key,
    Object.fromEntries(Object.entries(rows).filter(([id, row]) => JSON.stringify(row) !== JSON.stringify(champions[key][id]))),
]));
const formats = byID(Dex.formats.all().filter(f => /^\[Gen 9 Champions\]/.test(f.name)).map(f => {
    const validator = new TeamValidator(f.id);
    const r = validator.ruleTable;
    return [f.id, {
        minLevel: r.minLevel, maxTotalLevel: r.maxTotalLevel, minSourceGen: validator.minSourceGen,
        evLimit: r.evLimit, itemLimit: Number(r.valueRules.get('itemclause') || 0),
        ruleSources: Object.fromEntries(r),
        complexBans: [...r.complexBans.values()], complexTeamBans: [...r.complexTeamBans.values()],
    }];
}));
function lines(rows) {
    return '{\n' + Object.entries(rows).map(([id, row]) => JSON.stringify(id) + ': ' + JSON.stringify(row)).join(',\n') + '\n}';
}
writeFileSync(new URL('../data/champions-legality.json', import.meta.url),
    '{\n"revision": ' + JSON.stringify(revision) + ',\n"formats": ' + lines(formats) +
    ',\n"species": ' + lines(champions.species) + ',\n"learnsets": ' + lines(champions.learnsets) +
    ',\n"regmb": {\n"species": ' + lines(regmb.species) + ',\n"learnsets": ' + lines(regmb.learnsets) + '\n}\n}\n');

// The NatDex Draft format cross-checks each move with National Dex AG
// before falling back to the Champions learnset. Keep the extra source tables
// separate so constructing a regular BSS validator does not parse them.
const legacyDex = new TeamValidator('gen9nationaldexag').dex;
const legacy = project(legacyDex, true);
const transferableItems = [...new Set([7, 8, 9].flatMap(gen =>
    Dex.forGen(gen).items.all().filter(item => !item.isNonstandard).map(item => item.id)))].sort();
const missingMoves = championsDex.moves.all()
    .filter(move => move.isNonstandard === 'Unobtainable' && move.gen === championsDex.gen)
    .map(move => move.id);
const unsketchable = legacyDex.moves.all()
    .filter(move => move.flags.nosketch || move.isZ || move.isMax)
    .map(move => move.id);
writeFileSync(new URL('../data/champions-natdex-legality.json', import.meta.url),
    '{\n"revision": ' + JSON.stringify(revision) + ',\n"species": ' + lines(legacy.species) +
    ',\n"learnsets": ' + lines(legacy.learnsets) + ',\n"transferableItems": ' + JSON.stringify(transferableItems) +
    ',\n"missingMoves": ' + JSON.stringify(missingMoves) + ',\n"unsketchable": ' + JSON.stringify(unsketchable) + '\n}\n');
