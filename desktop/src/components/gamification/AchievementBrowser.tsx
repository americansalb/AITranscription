/**
 * AchievementBrowser - Paginated grid view of all achievements with filters
 */

import { useState, useEffect, useCallback } from "react";
import { AchievementCard, Achievement, RARITY_COLORS } from "./AchievementCard";
import { XPBar } from "./XPBar";
import { PrestigeTier } from "./TierBadge";

interface GamificationProgress {
  user_id: number;
  current_level: number;
  current_xp: number;
  xp_to_next_level: number;
  level_progress: number;
  lifetime_xp: number;
  prestige_tier: PrestigeTier;
  tier_color: string;
  tier_progress: {
    current_tier: string;
    next_tier: string | null;
    tier_start_xp: number;
    tier_end_xp: number | null;
    xp_in_tier: number;
    progress: number;
    color: string;
  };
  xp_multiplier: number;
  achievements: {
    unlocked: number;
    total: number;
    progress: number;
    by_rarity: {
      common: number;
      rare: number;
      epic: number;
      legendary: number;
    };
  };
}

interface AchievementListResponse {
  achievements: Achievement[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
}

interface AchievementBrowserProps {
  progress: GamificationProgress | null;
  fetchAchievements: (params: {
    category?: string;
    rarity?: string;
    page?: number;
    page_size?: number;
    unlocked_only?: boolean;
  }) => Promise<AchievementListResponse>;
  onClose?: () => void;
}

const CATEGORIES = [
  { value: "", label: "All Categories" },
  { value: "volume", label: "Volume" },
  { value: "streak", label: "Streaks" },
  { value: "speed", label: "Speed" },
  { value: "context", label: "Context" },
  { value: "formality", label: "Formality" },
  { value: "learning", label: "AI Training" },
  { value: "temporal", label: "Temporal" },
  { value: "records", label: "Records" },
  { value: "combo", label: "Combinations" },
  { value: "special", label: "Special" },
];

const RARITIES = [
  { value: "", label: "All Rarities" },
  { value: "common", label: "Common" },
  { value: "rare", label: "Rare" },
  { value: "epic", label: "Epic" },
  { value: "legendary", label: "Legendary" },
];

export function AchievementBrowser({
  progress,
  fetchAchievements,
  onClose,
}: AchievementBrowserProps) {
  const [achievements, setAchievements] = useState<Achievement[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Filters
  const [category, setCategory] = useState("");
  const [rarity, setRarity] = useState("");
  const [unlockedOnly, setUnlockedOnly] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  // Pagination
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);

  const PAGE_SIZE = 24;

  const loadAchievements = useCallback(async () => {
    setLoading(true);
    setError(null);

    try {
      const result = await fetchAchievements({
        category: category || undefined,
        rarity: rarity || undefined,
        page,
        page_size: PAGE_SIZE,
        unlocked_only: unlockedOnly,
      });

      setAchievements(result.achievements);
      setTotalPages(result.total_pages);
      setTotal(result.total);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load achievements");
    } finally {
      setLoading(false);
    }
  }, [fetchAchievements, category, rarity, page, unlockedOnly]);

  useEffect(() => {
    loadAchievements();
  }, [loadAchievements]);

  // Reset to page 1 when filters change
  useEffect(() => {
    setPage(1);
  }, [category, rarity, unlockedOnly]);

  // Filter by search query (client-side)
  const filteredAchievements = achievements.filter((a) => {
    if (!searchQuery) return true;
    const query = searchQuery.toLowerCase();
    return (
      a.name.toLowerCase().includes(query) ||
      a.description.toLowerCase().includes(query)
    );
  });

  return (
    <div className="achievement-browser flex flex-col h-full bg-gray-900 text-white">
      {/* Header */}
      <div className="flex-shrink-0 p-4 border-b border-gray-700">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-xl font-bold">Achievements</h2>
          {onClose && (
            <button
              onClick={onClose}
              className="p-2 rounded-lg hover:bg-gray-700 transition-colors"
            >
              <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          )}
        </div>

        {/* XP Progress */}
        {progress && (
          <XPBar
            currentXP={progress.current_xp}
            xpToNextLevel={progress.xp_to_next_level}
            level={progress.current_level}
            tier={progress.prestige_tier}
            lifetimeXP={progress.lifetime_xp}
            showTierProgress={true}
            tierProgress={progress.tier_progress}
          />
        )}

        {/* Achievement Stats */}
        {progress && (
          <div className="mt-4 grid grid-cols-5 gap-2">
            <div className="text-center p-2 rounded-lg bg-gray-800">
              <div className="text-lg font-bold">{progress.achievements.unlocked}</div>
              <div className="text-xs text-gray-400">Unlocked</div>
            </div>
            {(["common", "rare", "epic", "legendary"] as const).map((r) => (
              <div
                key={r}
                className="text-center p-2 rounded-lg"
                style={{ background: `${RARITY_COLORS[r].bg}` }}
              >
                <div className="text-lg font-bold" style={{ color: RARITY_COLORS[r].text }}>
                  {progress.achievements.by_rarity[r]}
                </div>
                <div className="text-xs capitalize" style={{ color: RARITY_COLORS[r].text }}>
                  {r}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Filters */}
      <div className="flex-shrink-0 p-4 border-b border-gray-700 space-y-3">
        {/* Search */}
        <div className="relative">
          <input
            type="text"
            placeholder="Search achievements..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full px-4 py-2 pl-10 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 focus:outline-none focus:border-blue-500"
          />
          <svg
            className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
        </div>

        {/* Filter row */}
        <div className="flex flex-wrap gap-3">
          <select
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            className="px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white focus:outline-none focus:border-blue-500"
          >
            {CATEGORIES.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>

          <select
            value={rarity}
            onChange={(e) => setRarity(e.target.value)}
            className="px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white focus:outline-none focus:border-blue-500"
          >
            {RARITIES.map((r) => (
              <option key={r.value} value={r.value}>
                {r.label}
              </option>
            ))}
          </select>

          <label className="flex items-center gap-2 px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg cursor-pointer">
            <input
              type="checkbox"
              checked={unlockedOnly}
              onChange={(e) => setUnlockedOnly(e.target.checked)}
              className="rounded border-gray-600 bg-gray-700 text-blue-500 focus:ring-blue-500"
            />
            <span className="text-sm">Unlocked only</span>
          </label>

          <div className="ml-auto text-sm text-gray-400 self-center">
            {total} achievements
          </div>
        </div>
      </div>

      {/* Achievement Grid */}
      <div className="flex-1 overflow-y-auto p-4">
        {loading ? (
          <div className="flex items-center justify-center h-48">
            <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500" />
          </div>
        ) : error ? (
          <div className="flex flex-col items-center justify-center h-48 text-red-400">
            <p>{error}</p>
            <button
              onClick={loadAchievements}
              className="mt-2 px-4 py-2 bg-red-500/20 hover:bg-red-500/30 rounded-lg transition-colors"
            >
              Retry
            </button>
          </div>
        ) : filteredAchievements.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-48 text-gray-500">
            <p>No achievements found</p>
            <p className="text-sm">Try adjusting your filters</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {filteredAchievements.map((achievement) => (
              <AchievementCard key={achievement.id} achievement={achievement} />
            ))}
          </div>
        )}
      </div>

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex-shrink-0 p-4 border-t border-gray-700 flex items-center justify-center gap-2">
          <button
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            disabled={page === 1}
            className="px-3 py-1.5 bg-gray-800 hover:bg-gray-700 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg transition-colors"
          >
            Previous
          </button>

          <div className="flex items-center gap-1">
            {Array.from({ length: Math.min(5, totalPages) }, (_, i) => {
              let pageNum: number;
              if (totalPages <= 5) {
                pageNum = i + 1;
              } else if (page <= 3) {
                pageNum = i + 1;
              } else if (page >= totalPages - 2) {
                pageNum = totalPages - 4 + i;
              } else {
                pageNum = page - 2 + i;
              }

              return (
                <button
                  key={pageNum}
                  onClick={() => setPage(pageNum)}
                  className={`w-8 h-8 rounded-lg transition-colors ${
                    page === pageNum
                      ? "bg-blue-500 text-white"
                      : "bg-gray-800 hover:bg-gray-700 text-gray-300"
                  }`}
                >
                  {pageNum}
                </button>
              );
            })}
          </div>

          <button
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            disabled={page === totalPages}
            className="px-3 py-1.5 bg-gray-800 hover:bg-gray-700 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg transition-colors"
          >
            Next
          </button>
        </div>
      )}
    </div>
  );
}
