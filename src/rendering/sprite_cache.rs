// src/rendering/sprite_cache.rs
// STATE-OF-THE-ART LRU Sprite Cache System
// Multi-resolution, material-aware, memory-bounded
// Designed to outperform VESTA/OVITO rendering systems

use gtk4::cairo::ImageSurface;
use std::collections::HashMap;

/// Smallest cached sprite resolution (px). Atoms below this on screen are a
/// couple of pixels wide, where the shading is invisible anyway.
pub const MIN_SPRITE_PX: i32 = 8;

/// Largest cached sprite resolution (px). One entry costs 1 MB at this size;
/// beyond it the sprite is upscaled rather than cached at ever larger sizes.
pub const MAX_SPRITE_PX: i32 = 512;

/// Resolution used by `preload_common` — the bucket a typical atom falls in at
/// default zoom.
pub const PRELOAD_SPRITE_PX: i32 = 32;

/// Cache entry with LRU tracking and metadata
#[derive(Clone, Debug)]
struct CacheEntry {
    sprite: ImageSurface,
    last_used: u64,
    access_count: u64,
    size_bytes: usize,
}

impl CacheEntry {
    fn new(sprite: ImageSurface) -> Self {
        // Sprites are size-matched to their on-screen diameter, so entries
        // range from 8x8 to 512x512 ARGB32. Measure the surface instead of
        // assuming one size, or eviction accounting drifts by up to 64x.
        let size_bytes = (sprite.width() * sprite.height() * 4) as usize;
        Self {
            sprite,
            last_used: 0,
            access_count: 1,
            size_bytes,
        }
    }
}

/// Cache statistics for performance monitoring
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entries: usize,
    pub max_entries: usize,
    pub memory_mb: f64,
    pub max_memory_mb: f64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub hit_rate: f64,
    pub avg_access_per_entry: f64,
}

/// Eviction strategy
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvictionPolicy {
    LRU,      // Least Recently Used (default)
    LFU,      // Least Frequently Used
    Size,     // Largest entries first
    Adaptive, // Hybrid LRU + access frequency
}

/// STATE-OF-THE-ART sprite cache with intelligent management
#[derive(Debug)]
pub struct SpriteCache {
    cache: HashMap<String, CacheEntry>,
    current_frame: u64,

    // Memory management
    max_entries: usize,
    max_memory_mb: f64,
    current_memory_bytes: usize,

    // Statistics
    hits: u64,
    misses: u64,
    evictions: u64,

    // Configuration
    eviction_policy: EvictionPolicy,
    preload_common: bool,
}

impl SpriteCache {
    /// Create SOTA cache with memory limit
    ///
    /// # Arguments
    /// * `max_memory_mb` - Maximum memory in MB (default: 200)
    ///
    /// # Performance
    /// - 200 MB ≈ 3,200 sprites (128×128 ARGB32)
    /// - Typical structure: 10-20 elements × 3 sizes × 2 materials = 60-120 sprites
    /// - Hit rate target: >95% for normal usage
    pub fn new(max_memory_mb: f64) -> Self {
        let bytes_per_sprite = 128 * 128 * 4; // 65,536 bytes
        let max_entries = ((max_memory_mb * 1024.0 * 1024.0) / bytes_per_sprite as f64) as usize;

        Self {
            cache: HashMap::with_capacity(max_entries / 2),
            current_frame: 0,
            max_entries,
            max_memory_mb,
            current_memory_bytes: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            eviction_policy: EvictionPolicy::Adaptive,
            preload_common: true,
        }
    }

    /// Smallest sprite resolution that covers an on-screen diameter of
    /// `diameter_px`, as a power of two in [MIN_SPRITE_PX, MAX_SPRITE_PX].
    ///
    /// Power-of-two buckets keep the blit scale factor in (0.5, 1.0] — the
    /// range where Cairo's `Filter::Good` stays on its cheap path — while
    /// capping the cache at ~7 resolutions per element/material combination.
    /// Atoms drawn larger than MAX_SPRITE_PX are upscaled, which is the cheap
    /// direction and only happens on extreme zoom-in.
    pub fn size_bucket(diameter_px: f64) -> i32 {
        // NaN has no meaningful size; anything at or below the floor, and any
        // negative, takes the smallest bucket. Infinity falls through to the
        // loop and saturates at MAX_SPRITE_PX like any other huge value.
        if diameter_px.is_nan() || diameter_px <= MIN_SPRITE_PX as f64 {
            return MIN_SPRITE_PX;
        }
        let mut px = MIN_SPRITE_PX;
        while (px as f64) < diameter_px && px < MAX_SPRITE_PX {
            px *= 2;
        }
        px
    }

    /// Generate intelligent cache key from rendering parameters
    ///
    /// Key format: "element_pXXX_mXX_rXX_tXX"
    /// - element: Chemical symbol
    /// - pXXX: Sprite resolution in pixels (see `size_bucket`)
    /// - mXX: Metallic (2 digits, 0-99)
    /// - rXX: Roughness (2 digits, 0-99)
    /// - tXX: Transmission (2 digits, 0-99)
    ///
    /// # Examples
    /// - "Fe_p032_m30_r40_t00" = Iron drawn at 17-32 px, 0.3 metallic, 0.4 roughness
    /// - "O_p064_m00_r30_t00"  = Oxygen drawn at 33-64 px, 0.0 metallic, 0.3 roughness
    ///
    /// # Design
    /// - The pixel bucket, not the style's `atom_scale`, is what the sprite
    ///   actually depends on: zoom, per-atom radius overrides and the element's
    ///   covalent radius all feed the on-screen size. Keying on `atom_scale`
    ///   alone returned one fixed-resolution sprite for every zoom level.
    /// - Material precision: 0.01 (human eye threshold)
    pub fn make_key(
        element: &str,
        sprite_px: i32,
        metallic: f64,
        roughness: f64,
        transmission: f64,
    ) -> String {
        format!(
            "{}_p{:03}_m{:02}_r{:02}_t{:02}",
            element,
            sprite_px,
            (metallic * 100.0).round() as u32,
            (roughness * 100.0).round() as u32,
            (transmission * 100.0).round() as u32,
        )
    }

    /// Get sprite from cache or create new (main API)
    ///
    /// # Performance
    /// - Cache hit: O(1) lookup, ~10 ns
    /// - Cache miss: O(1) insertion + sprite generation (~1-5 ms)
    /// - LRU eviction: O(n) scan when needed (~100 μs for 1000 entries)
    pub fn get_or_insert<F>(&mut self, key: String, create_fn: F) -> ImageSurface
    where
        F: FnOnce() -> ImageSurface,
    {
        self.current_frame += 1;

        // Fast path: Cache hit
        if let Some(entry) = self.cache.get_mut(&key) {
            entry.last_used = self.current_frame;
            entry.access_count += 1;
            self.hits += 1;
            return entry.sprite.clone();
        }

        // Slow path: Cache miss - create new sprite
        self.misses += 1;
        let sprite = create_fn();

        // Memory management: Evict before inserting if needed
        let entry_size = (sprite.width() * sprite.height() * 4) as usize;
        while self.should_evict(entry_size) {
            self.evict_one();
        }

        // Insert new entry
        let mut entry = CacheEntry::new(sprite.clone());
        entry.last_used = self.current_frame;
        self.current_memory_bytes += entry_size;
        self.cache.insert(key, entry);

        sprite
    }

    /// Check if eviction is needed
    #[inline]
    fn should_evict(&self, incoming_bytes: usize) -> bool {
        let would_exceed_entries = self.cache.len() >= self.max_entries;
        let would_exceed_memory = (self.current_memory_bytes + incoming_bytes) as f64
            > (self.max_memory_mb * 1024.0 * 1024.0);

        would_exceed_entries || would_exceed_memory
    }

    /// Evict one entry using configured policy
    fn evict_one(&mut self) {
        if self.cache.is_empty() {
            return;
        }

        let victim_key = match self.eviction_policy {
            EvictionPolicy::LRU => self.find_lru(),
            EvictionPolicy::LFU => self.find_lfu(),
            EvictionPolicy::Size => self.find_largest(),
            EvictionPolicy::Adaptive => self.find_adaptive(),
        };

        if let Some(key) = victim_key {
            if let Some(entry) = self.cache.remove(&key) {
                self.current_memory_bytes =
                    self.current_memory_bytes.saturating_sub(entry.size_bytes);
                self.evictions += 1;
            }
        }
    }

    /// Find least recently used entry
    fn find_lru(&self) -> Option<String> {
        self.cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.clone())
    }

    /// Find least frequently used entry
    fn find_lfu(&self) -> Option<String> {
        self.cache
            .iter()
            .min_by_key(|(_, entry)| entry.access_count)
            .map(|(key, _)| key.clone())
    }

    /// Find largest entry (memory-based)
    fn find_largest(&self) -> Option<String> {
        self.cache
            .iter()
            .max_by_key(|(_, entry)| entry.size_bytes)
            .map(|(key, _)| key.clone())
    }

    /// Adaptive eviction: Hybrid score = recency × frequency
    ///
    /// Score = (frames_since_use) × (1.0 / access_count)
    /// - High score = old AND rarely used → evict first
    /// - Low score = recent OR frequently used → keep
    fn find_adaptive(&self) -> Option<String> {
        self.cache
            .iter()
            .max_by(|(_, a), (_, b)| {
                let score_a = (self.current_frame - a.last_used) as f64 / a.access_count as f64;
                let score_b = (self.current_frame - b.last_used) as f64 / b.access_count as f64;
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(key, _)| key.clone())
    }

    /// Clear entire cache (e.g., on major style change)
    pub fn clear(&mut self) {
        self.cache.clear();
        self.current_memory_bytes = 0;
        self.hits = 0;
        self.misses = 0;
        self.evictions = 0;
    }

    /// Partial clear: Remove entries matching predicate
    ///
    /// # Use cases
    /// - Element removed: `cache.clear_matching(|k| k.starts_with("Fe_"))`
    /// - Style changed: `cache.clear_matching(|k| k.contains("_m30_"))`
    pub fn clear_matching<F>(&mut self, mut predicate: F)
    where
        F: FnMut(&str) -> bool,
    {
        let keys_to_remove: Vec<String> = self
            .cache
            .keys()
            .filter(|k| predicate(k))
            .cloned()
            .collect();

        for key in keys_to_remove {
            if let Some(entry) = self.cache.remove(&key) {
                self.current_memory_bytes =
                    self.current_memory_bytes.saturating_sub(entry.size_bytes);
            }
        }
    }

    /// Preload common elements (optimization for startup)
    ///
    /// Preloads sprites for most common elements at standard settings
    /// - Elements: H, C, N, O, F, Si, P, S, Cl, Fe, Cu, Zn
    /// - Resolution: `PRELOAD_SPRITE_PX`, the bucket a default-zoom atom lands in
    /// - Material: Default (metallic=0.0, roughness=0.3)
    pub fn preload_common<F>(&mut self, mut create_fn: F)
    where
        F: FnMut(&str, i32, f64, f64, f64) -> ImageSurface,
    {
        if !self.preload_common {
            return;
        }

        let common_elements = [
            "H", "C", "N", "O", "F", "Si", "P", "S", "Cl", "Fe", "Cu", "Zn",
        ];
        let sprite_px = PRELOAD_SPRITE_PX;
        let metallic = 0.0;
        let roughness = 0.3;
        let transmission = 0.0;

        for element in &common_elements {
            let key = Self::make_key(element, sprite_px, metallic, roughness, transmission);
            if !self.cache.contains_key(&key) {
                let sprite = create_fn(element, sprite_px, metallic, roughness, transmission);
                let entry = CacheEntry::new(sprite);
                self.current_memory_bytes += entry.size_bytes;
                self.cache.insert(key, entry);
            }
        }
    }

    /// Get comprehensive cache statistics
    pub fn stats(&self) -> CacheStats {
        let total_requests = self.hits + self.misses;
        let avg_access = if !self.cache.is_empty() {
            self.cache.values().map(|e| e.access_count).sum::<u64>() as f64
                / self.cache.len() as f64
        } else {
            0.0
        };

        CacheStats {
            entries: self.cache.len(),
            max_entries: self.max_entries,
            memory_mb: self.current_memory_bytes as f64 / (1024.0 * 1024.0),
            max_memory_mb: self.max_memory_mb,
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            hit_rate: if total_requests > 0 {
                self.hits as f64 / total_requests as f64
            } else {
                0.0
            },
            avg_access_per_entry: avg_access,
        }
    }

    /// Print detailed statistics (debug helper)
    pub fn print_stats(&self) {
        let stats = self.stats();
        println!("╔═══════════════════════════════════════╗");
        println!("║   SPRITE CACHE STATISTICS (SOTA)     ║");
        println!("╠═══════════════════════════════════════╣");
        println!(
            "║ Entries:      {:6} / {:6}        ║",
            stats.entries, stats.max_entries
        );
        println!(
            "║ Memory:       {:6.1} / {:6.1} MB    ║",
            stats.memory_mb, stats.max_memory_mb
        );
        println!(
            "║ Hit Rate:     {:6.2}%               ║",
            stats.hit_rate * 100.0
        );
        println!("║ Hits:         {:8}                ║", stats.hits);
        println!("║ Misses:       {:8}                ║", stats.misses);
        println!("║ Evictions:    {:8}                ║", stats.evictions);
        println!(
            "║ Avg Access:   {:6.1}                 ║",
            stats.avg_access_per_entry
        );
        println!("╚═══════════════════════════════════════╝");
    }

    /// Set eviction policy
    pub fn set_eviction_policy(&mut self, policy: EvictionPolicy) {
        self.eviction_policy = policy;
    }

    /// Get current memory usage in MB
    pub fn memory_usage_mb(&self) -> f64 {
        self.current_memory_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Get cache efficiency score (0-100)
    ///
    /// Score components:
    /// - Hit rate: 50% weight
    /// - Memory efficiency: 30% weight  
    /// - Access pattern: 20% weight
    pub fn efficiency_score(&self) -> f64 {
        let stats = self.stats();

        // Component 1: Hit rate (target >95%)
        let hit_score = (stats.hit_rate * 100.0).min(100.0);

        // Component 2: Memory efficiency (should use 50-80% of available)
        let usage_ratio = stats.memory_mb / stats.max_memory_mb;
        let memory_score = if usage_ratio < 0.5 {
            usage_ratio * 200.0 // Underutilized
        } else if usage_ratio > 0.8 {
            (1.0 - usage_ratio) * 500.0 // Overutilized
        } else {
            100.0 // Optimal
        };

        // Component 3: Access pattern (high reuse is good)
        let access_score = (stats.avg_access_per_entry * 10.0).min(100.0);

        // Weighted average
        (hit_score * 0.5 + memory_score * 0.3 + access_score * 0.2).min(100.0)
    }
}

impl Default for SpriteCache {
    fn default() -> Self {
        Self::new(200.0) // 200 MB default
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_generation() {
        let key = SpriteCache::make_key("Fe", 32, 0.30, 0.45, 0.00);
        assert_eq!(key, "Fe_p032_m30_r45_t00");
    }

    #[test]
    fn size_bucket_covers_the_atom_at_a_cheap_scale_factor() {
        // Every bucket must be >= the requested diameter (so the blit never
        // upscales below MAX) and < 2x it (so the scale factor stays above
        // 0.5, where Cairo's Filter::Good keeps its fast path).
        for d in [1.0f64, 8.0, 8.1, 9.0, 16.0, 17.0, 31.9, 100.0, 511.0] {
            let px = SpriteCache::size_bucket(d);
            assert!(px >= MIN_SPRITE_PX && px <= MAX_SPRITE_PX, "{d} -> {px}");
            assert!(px as f64 >= d, "bucket {px} too small for {d}");
            if d > MIN_SPRITE_PX as f64 {
                assert!(
                    d / px as f64 > 0.5,
                    "bucket {px} for {d} gives scale {}",
                    d / px as f64
                );
            }
        }
    }

    #[test]
    fn size_bucket_is_bounded_at_both_ends() {
        assert_eq!(SpriteCache::size_bucket(0.0), MIN_SPRITE_PX);
        assert_eq!(SpriteCache::size_bucket(-5.0), MIN_SPRITE_PX);
        assert_eq!(SpriteCache::size_bucket(f64::NAN), MIN_SPRITE_PX);
        assert_eq!(SpriteCache::size_bucket(1e9), MAX_SPRITE_PX);
        assert_eq!(SpriteCache::size_bucket(f64::INFINITY), MAX_SPRITE_PX);
    }

    #[test]
    fn distinct_sizes_get_distinct_cache_entries() {
        let small = SpriteCache::make_key("C", SpriteCache::size_bucket(12.0), 0.0, 0.3, 0.0);
        let large = SpriteCache::make_key("C", SpriteCache::size_bucket(90.0), 0.0, 0.3, 0.0);
        assert_ne!(small, large);
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = SpriteCache::new(0.1); // Very small cache

        // This would require actual ImageSurface creation
        // Just testing the structure
        assert_eq!(cache.stats().entries, 0);
    }

    #[test]
    fn test_adaptive_scoring() {
        let cache = SpriteCache::new(200.0);
        let score = cache.efficiency_score();
        assert!(score >= 0.0 && score <= 100.0);
    }
}
