# World Generation Roadmap

## Progress

- [x] Foundation
- [x] Noise System
- [x] Biomes
- [x] Terrain
- [x] Surface
- [x] Carvers
- [x] Ores
- [x] Liquids
- [ ] Structures
- [x] Decorations
- [x] Trees
- [x] Snow & Ice
- [x] Spawn
- [x] Optimization
- [ ] API
- [x] Testing

---

# Foundation

## Random

- [x] World Seed
- [x] Chunk Seed
- [x] Deterministic RNG
- [x] Coordinate Hashing

## Noise

- [x] Perlin Noise
- [x] Simplex Noise (canal `clay`)
- [x] Value Noise (canal `patch`)
- [x] FBM
- [x] Octaves
- [x] Interpolation
- [ ] Noise Cache

---

# Biomes

## Climate

- [x] Temperature
- [x] Humidity
- [x] Continentalness
- [ ] Weirdness
- [x] Erosion

## Biome Selection

- [x] Plains
- [x] Forest
- [x] Taiga
- [x] Jungle
- [x] Desert
- [x] Savanna
- [x] Swamp
- [x] Mountains
- [x] Ocean
- [x] Deep Ocean
- [x] Mushroom Island

## Transitions

- [x] Rivers
- [x] Beaches
- [ ] Shore
- [ ] Hills
- [ ] Smoothing

---

# Terrain

## Heightmap

- [x] Base Height
- [x] Height Variation
- [x] Mountains
- [x] Valleys
- [ ] Cliffs
- [ ] Blending

## Terrain Fill

- [x] Stone
- [x] Air
- [x] Water

---

# Surface

- [x] Grass
- [x] Dirt
- [x] Sand
- [x] Gravel
- [x] Clay
- [x] Snow
- [x] Ice
- [x] Mycelium

---

# Carvers

- [x] Caves
- [x] Ravines
- [ ] Cave Networks

---

# Ores

## Overworld

- [x] Coal
- [x] Iron
- [x] Gold
- [x] Redstone
- [x] Diamond
- [x] Emerald
- [x] Lapis

## Nether

- [ ] Quartz

---

# Liquids

- [x] Water Springs
- [x] Lava Springs
- [x] Lakes
- [x] Waterfalls

---

# Structures

- [ ] Villages
- [ ] Strongholds
- [ ] Mineshafts
- [ ] Dungeons
- [ ] Desert Temple
- [ ] Jungle Temple
- [ ] Witch Hut
- [ ] Nether Fortress
- [ ] Bonus Chest

---

# Decorations

## Vegetation

- [x] Tall Grass
- [x] Flowers
- [x] Mushrooms
- [x] Sugar Cane
- [x] Cactus
- [x] Dead Bush
- [x] Vines
- [x] Lily Pads

## Misc

- [x] Pumpkins
- [x] Melons

---

# Trees

- [x] Oak
- [x] Large Oak
- [x] Birch
- [x] Spruce
- [x] Pine
- [ ] Mega Taiga
- [x] Jungle
- [x] Large Jungle
- [x] Swamp
- [x] Acacia
- [x] Dark Oak

---

# Snow & Ice

- [x] Snow Layer
- [x] Water Freezing

---

# Spawn

- [x] Spawn Search
- [x] Spawn Validation

---

# Optimization

- [ ] Biome Cache
- [ ] Noise Cache
- [x] Parallel Generation
- [ ] Async Generation
- [ ] SIMD
- [ ] Chunk Pipeline

---

# API

- [ ] BiomeProvider
- [ ] TerrainGenerator
- [ ] SurfaceGenerator
- [ ] Carver
- [ ] FeatureGenerator
- [ ] StructureGenerator
- [ ] Decorator

---

# Testing

- [ ] Chunk Borders
- [x] Determinism
- [x] Infinite Generation
- [x] Performance
- [ ] Stress Test

---

# Future Ideas

- [ ] Custom Biomes
- [ ] Custom Structures
- [ ] Custom World Types
- [ ] Multithreaded Decoration
- [ ] World Presets
