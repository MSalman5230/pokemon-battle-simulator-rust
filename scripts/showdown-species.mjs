// The inherited Pokédex lists mechanically identical cosmetic formes on
// their base species. species.get resolves names that species.all omits.
export function speciesIncludingCosmetics(dex) {
    const base = dex.species.all();
    const entries = new Map(base.map(species => [species.id, species]));
    for (const species of base) {
        for (const name of species.cosmeticFormes ?? []) {
            const forme = dex.species.get(name);
            if (forme.exists && !entries.has(forme.id)) entries.set(forme.id, forme);
        }
    }
    return [...entries.values()];
}
