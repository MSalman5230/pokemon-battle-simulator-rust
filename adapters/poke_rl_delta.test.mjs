import assert from 'node:assert/strict';
import test from 'node:test';
import {DeltaDecoder} from './poke_rl_delta.mjs';

const frame = (key, sequence, p1, p2) => ({
    $battle_delta_v1: [
        key, sequence, [p1, p2], [false, false, null],
        ...(sequence === 0 ? [[[{'item': 'Luca berry'}], [{'item': 'Silk scarf'}]]] : []),
    ],
});
const seed = tag => [
    {turn: 1}, [0, {species: tag}], [0, {seen: false}], [0, {move: 'tackle'}],
];
const unchanged = [{}, [], [], null];

test('deltas preserve previous observations and isolate game keys', () => {
    const decoder = new DeltaDecoder();
    const first = decoder.expand(frame('a', 0, seed('A1'), seed('A2')));
    const [second, other] = decoder.expand([
        frame('a', 1, [{turn: 2}, [0, {species: 'A1', hp: 0.5}], [], [1, {move: 'protect'}]], unchanged),
        frame('b', 0, seed('B1'), seed('B2')),
    ]);
    assert.deepEqual(first.views[0].history, [{move: 'tackle'}]);
    assert.deepEqual(first.views[0].own, [{species: 'A1', set: {item: 'Luca berry'}}]);
    assert.deepEqual(second.views[0].history, [{move: 'protect'}]);
    assert.equal(second.views[0].own[0].hp, 0.5);
    assert.strictEqual(second.views[0].own[0].set, first.views[0].own[0].set);
    assert.strictEqual(second.training.critic_views[1].opponent, second.views[0].own);
    assert.equal(other.training.critic_views[1].opponent[0].species, 'B1');
});

test('close, missing sequences, and resets cannot use a stale base', () => {
    const decoder = new DeltaDecoder();
    decoder.expand(frame('slot', 0, seed('first'), seed('foe')));
    assert.throws(() => decoder.expand(frame('slot', 2, unchanged, unchanged)), /Missing observation base/);
    assert.equal(decoder.expand({$battle_closed: 'slot'}), true);
    assert.throws(() => decoder.expand(frame('slot', 1, unchanged, unchanged)), /Missing observation base/);
    decoder.expand(frame('slot', 0, seed('reset'), seed('foe')));
    decoder.clear();
    assert.throws(() => decoder.expand(frame('slot', 1, unchanged, unchanged)), /Missing observation base/);
});

test('packed slot updates preserve old snapshots and distinguish absent from null', () => {
    const decoder = new DeltaDecoder();
    const slot = (present, changed, ...values) => ({present, changed, values});
    const row = ({present, changed, values}) => [present, changed, ...values];
    const base = frame('packed', 0, [
        {turn: 1, legal: 1},
        [0, row(slot(7, 7, 'Espeon', 'Espeon', 1))],
        [0, row(slot(3, 3, 'Umbreon', 'Umbreon'))],
        [0, [1, 0, 'tackle', 0, 'Espeon', 0]],
    ], [{turn: 1, legal: []}, [], [], null]);
    const first = decoder.expand({$battle_delta_v2: base.$battle_delta_v1});
    const next = frame('packed', 1, [
        {turn: 2, legal: [[0, 0, 'tackle', false]]},
        [0, row(slot(5, 4, null))],
        [], [1, [2, 0, 'tackle', 0, 'Espeon', 0]],
    ], unchanged);
    const second = decoder.expand({$battle_delta_v2: next.$battle_delta_v1});
    assert.equal(first.views[0].own[0].hp, 1);
    assert.equal(first.views[0].own[0].species, 'Espeon');
    assert.equal(first.views[0].legal.length, 120);
    assert.equal(second.views[0].own[0].hp, null);
    assert(!Object.hasOwn(second.views[0].own[0], 'species'));
    assert.equal(second.views[0].legal[0].choice, 'move 1');
    assert.deepEqual(second.views[0].history, [{turn: 2, side: 0, move: 'tackle', actor: 0, species: 'Espeon', order: 0}]);
});

test('packed move columns carry independent PP updates and allow removals', () => {
    const decoder = new DeltaDecoder();
    const start = frame('moves', 0,
        [{turn: 1, legal: []}, [0, [0xe0000020, 0xe0000020,
            ['tackle', 20, 20, false], ['protect', 10, 10, false],
            ['rest', 5, 5, false], ['growl', 20, 20, false]]], [], null],
        [{turn: 1, legal: []}, [], [], null]);
    const first = decoder.expand({$battle_delta_v2: start.$battle_delta_v1});
    const next = frame('moves', 1,
        [{turn: 2}, [0, [0x60000020, 0x20000000, ['protect', 9, 10, true]]], [], null],
        unchanged);
    const second = decoder.expand({$battle_delta_v2: next.$battle_delta_v1});
    assert.deepEqual(first.views[0].own[0].moves.map(m => m.pp), [20, 10, 5, 20]);
    assert.deepEqual(second.views[0].own[0].moves.map(m => m.pp), [20, 9, 5]);
    assert.equal(second.views[0].own[0].moves[1].disabled, true);
});
