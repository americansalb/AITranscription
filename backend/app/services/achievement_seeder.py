"""Achievement seeder - generates all 1,100+ achievement definitions."""

from typing import Any

from app.models.gamification import AchievementCategory, AchievementRarity


def roman_numeral(num: int) -> str:
    """Convert integer to Roman numeral."""
    val = [1000, 900, 500, 400, 100, 90, 50, 40, 10, 9, 5, 4, 1]
    syms = ["M", "CM", "D", "CD", "C", "XC", "L", "XL", "X", "IX", "V", "IV", "I"]
    roman_num = ""
    for i, v in enumerate(val):
        while num >= v:
            roman_num += syms[i]
            num -= v
    return roman_num


def get_rarity(tier: int, max_tier: int) -> AchievementRarity:
    """Determine rarity based on tier position."""
    pct = tier / max_tier
    if pct <= 0.25:
        return AchievementRarity.COMMON
    elif pct <= 0.50:
        return AchievementRarity.RARE
    elif pct <= 0.75:
        return AchievementRarity.EPIC
    return AchievementRarity.LEGENDARY


def calculate_xp(category: str, tier: int, rarity: AchievementRarity) -> int:
    """Calculate XP reward for an achievement."""
    base = {
        "volume": 10, "streak": 15, "speed": 12, "context": 8,
        "formality": 10, "learning": 20, "temporal": 8,
        "records": 25, "combo": 30, "special": 50
    }
    rarity_mult = {
        AchievementRarity.COMMON: 1,
        AchievementRarity.RARE: 1.5,
        AchievementRarity.EPIC: 2.5,
        AchievementRarity.LEGENDARY: 5
    }
    tier_mult = 1 + (tier - 1) * 0.5 + (tier / 10) ** 2
    # H1 fix: Guarantee minimum 5 XP — low-tier achievements should never round to 0
    return max(5, round(base.get(category, 10) * tier_mult * rarity_mult[rarity] / 5) * 5)


def generate_tiered_achievements(
    id_prefix: str,
    name_template: str,
    description_template: str,
    category: AchievementCategory,
    metric_type: str,
    thresholds: list[float],
    icon: str,
    is_hidden: bool = False,
) -> list[dict[str, Any]]:
    """Generate a tiered achievement line."""
    achievements = []
    max_tier = len(thresholds)

    for i, threshold in enumerate(thresholds, 1):
        tier_roman = roman_numeral(i)
        rarity = get_rarity(i, max_tier)
        xp = calculate_xp(category.value, i, rarity)

        # Format threshold for description
        if threshold >= 1_000_000:
            thresh_str = f"{threshold / 1_000_000:.1f}M".replace(".0M", "M")
        elif threshold >= 1_000:
            thresh_str = f"{threshold / 1_000:.1f}K".replace(".0K", "K")
        else:
            thresh_str = str(int(threshold))

        achievements.append({
            "id": f"{id_prefix}_{i}",
            "name": name_template.format(tier=tier_roman),
            "description": description_template.format(threshold=thresh_str, value=int(threshold)),
            "category": category,
            "rarity": rarity,
            "xp_reward": xp,
            "icon": icon,
            "tier": i,
            "threshold": threshold,
            "metric_type": metric_type,
            "is_hidden": is_hidden,
            "parent_id": f"{id_prefix}_{i-1}" if i > 1 else None,
        })

    return achievements


def generate_all_achievements() -> list[dict[str, Any]]:
    """Generate all 1,100+ achievement definitions."""
    achievements = []

    # =============================================================================
    # VOLUME ACHIEVEMENTS (200 total)
    # Icon identifiers: words, mic, clock, text, calendar, chart
    # =============================================================================

    # Word Count (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_words",
        name_template="Word Warrior {tier}",
        description_template="Transcribe {threshold} words total",
        category=AchievementCategory.VOLUME,
        metric_type="total_words",
        thresholds=[100, 250, 500, 1000, 2500, 5000, 10000, 25000, 50000, 75000,
                   100000, 150000, 250000, 500000, 750000, 1000000, 2500000,
                   5000000, 7500000, 10000000],
        icon="words",
    ))

    # Transcription Count (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_trans",
        name_template="Transcription Master {tier}",
        description_template="Complete {threshold} transcriptions",
        category=AchievementCategory.VOLUME,
        metric_type="total_transcriptions",
        thresholds=[5, 10, 25, 50, 100, 200, 350, 500, 750, 1000,
                   1500, 2000, 3000, 5000, 7500, 10000, 15000, 25000, 50000, 100000],
        icon="mic",
    ))

    # Audio Time (20 tiers) - in seconds
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_audio",
        name_template="Audio Explorer {tier}",
        description_template="Transcribe {value} seconds of audio",
        category=AchievementCategory.VOLUME,
        metric_type="total_audio_seconds",
        thresholds=[60, 300, 600, 1800, 3600, 7200, 14400, 28800, 43200, 72000,
                   108000, 180000, 360000, 540000, 720000, 1080000, 1800000,
                   3600000, 7200000, 18000000],
        icon="waveform",
    ))

    # Characters Transcribed (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_chars",
        name_template="Character Count {tier}",
        description_template="Transcribe {threshold} characters",
        category=AchievementCategory.VOLUME,
        metric_type="total_characters",
        thresholds=[500, 1000, 2500, 5000, 10000, 25000, 50000, 100000, 250000, 500000,
                   750000, 1000000, 2500000, 5000000, 7500000, 10000000, 25000000,
                   50000000, 75000000, 100000000],
        icon="text",
    ))

    # Daily Word Records (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_daily_words",
        name_template="Daily Output {tier}",
        description_template="Transcribe {threshold} words in a single day",
        category=AchievementCategory.VOLUME,
        metric_type="daily_word_record",
        thresholds=[100, 250, 500, 1000, 1500, 2000, 3000, 5000, 7500, 10000,
                   15000, 20000, 30000, 40000, 50000, 75000, 100000, 150000, 200000, 300000],
        icon="calendar-day",
    ))

    # Weekly Word Records (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_weekly_words",
        name_template="Weekly Output {tier}",
        description_template="Transcribe {threshold} words in a single week",
        category=AchievementCategory.VOLUME,
        metric_type="weekly_word_record",
        thresholds=[500, 1000, 2500, 5000, 10000, 15000, 25000, 40000, 60000, 80000,
                   100000, 150000, 200000, 300000, 400000, 500000, 750000, 1000000, 1500000, 2000000],
        icon="calendar-week",
    ))

    # Monthly Word Records (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_monthly_words",
        name_template="Monthly Output {tier}",
        description_template="Transcribe {threshold} words in a single month",
        category=AchievementCategory.VOLUME,
        metric_type="monthly_word_record",
        thresholds=[1000, 2500, 5000, 10000, 25000, 50000, 75000, 100000, 150000, 200000,
                   300000, 400000, 500000, 750000, 1000000, 1500000, 2000000, 3000000, 5000000, 10000000],
        icon="calendar-month",
    ))

    # Daily Transcription Count (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_daily_trans",
        name_template="Daily Sessions {tier}",
        description_template="Complete {value} transcriptions in a single day",
        category=AchievementCategory.VOLUME,
        metric_type="daily_transcription_record",
        thresholds=[5, 10, 15, 20, 30, 40, 50, 75, 100, 125,
                   150, 200, 250, 300, 400, 500, 750, 1000, 1500, 2000],
        icon="layers",
    ))

    # Session Word Count (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_session",
        name_template="Session Volume {tier}",
        description_template="Transcribe {threshold} words in a single session",
        category=AchievementCategory.VOLUME,
        metric_type="session_word_record",
        thresholds=[100, 250, 500, 750, 1000, 1500, 2000, 3000, 4000, 5000,
                   7500, 10000, 15000, 20000, 25000, 35000, 50000, 75000, 100000, 150000],
        icon="document",
    ))

    # Polished Words (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="vol_polished",
        name_template="Polish Volume {tier}",
        description_template="Polish {threshold} words total",
        category=AchievementCategory.VOLUME,
        metric_type="total_polished_words",
        thresholds=[100, 250, 500, 1000, 2500, 5000, 10000, 25000, 50000, 75000,
                   100000, 150000, 250000, 500000, 750000, 1000000, 2500000,
                   5000000, 7500000, 10000000],
        icon="sparkle",
    ))

    # =============================================================================
    # STREAK/CONSISTENCY ACHIEVEMENTS (120 total)
    # =============================================================================

    # Current Streak (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="streak_current",
        name_template="Streak {tier}",
        description_template="Maintain a {value}-day streak",
        category=AchievementCategory.STREAK,
        metric_type="current_streak",
        thresholds=[3, 5, 7, 10, 14, 21, 30, 45, 60, 90,
                   120, 150, 180, 250, 365, 500, 730, 1000, 1500, 2000],
        icon="flame",
    ))

    # Longest Streak (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="streak_longest",
        name_template="Best Streak {tier}",
        description_template="Achieve a longest streak of {value} days",
        category=AchievementCategory.STREAK,
        metric_type="longest_streak",
        thresholds=[7, 14, 21, 30, 45, 60, 90, 120, 150, 180,
                   250, 365, 500, 730, 1000, 1500, 2000, 2500, 3000, 3650],
        icon="trophy",
    ))

    # Total Active Days (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="streak_active",
        name_template="Active Days {tier}",
        description_template="Be active on {value} different days",
        category=AchievementCategory.STREAK,
        metric_type="total_active_days",
        thresholds=[5, 10, 25, 50, 75, 100, 150, 200, 300, 365,
                   500, 730, 1000, 1500, 2000, 2500, 3000, 3650, 5000, 7300],
        icon="chart-bar",
    ))

    # Perfect Weeks (20 tiers) - 7 consecutive days
    achievements.extend(generate_tiered_achievements(
        id_prefix="streak_perfect_week",
        name_template="Perfect Week {tier}",
        description_template="Complete {value} perfect weeks (7/7 days active)",
        category=AchievementCategory.STREAK,
        metric_type="perfect_weeks",
        thresholds=[1, 2, 4, 8, 12, 16, 24, 36, 52, 78,
                   104, 156, 208, 260, 312, 416, 520, 730, 1000, 1460],
        icon="calendar-check",
    ))

    # Perfect Months (20 tiers) - 30 consecutive days
    achievements.extend(generate_tiered_achievements(
        id_prefix="streak_perfect_month",
        name_template="Perfect Month {tier}",
        description_template="Complete {value} perfect months (30/30 days active)",
        category=AchievementCategory.STREAK,
        metric_type="perfect_months",
        thresholds=[1, 2, 3, 4, 6, 8, 10, 12, 18, 24,
                   36, 48, 60, 72, 84, 96, 120, 180, 240, 365],
        icon="medal",
    ))

    # Comeback Streaks (20 tiers) - returning after absence
    achievements.extend(generate_tiered_achievements(
        id_prefix="streak_comeback",
        name_template="Comeback {tier}",
        description_template="Return from a {value}+ day break and start a new streak",
        category=AchievementCategory.STREAK,
        metric_type="comeback_count",
        thresholds=[1, 2, 3, 5, 7, 10, 15, 20, 25, 30,
                   40, 50, 75, 100, 150, 200, 300, 500, 750, 1000],
        icon="refresh",
    ))

    # =============================================================================
    # SPEED ACHIEVEMENTS (80 total)
    # =============================================================================

    # Fastest WPM Record (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="speed_fastest",
        name_template="Speed Record {tier}",
        description_template="Achieve a transcription with {value}+ WPM",
        category=AchievementCategory.SPEED,
        metric_type="fastest_wpm",
        thresholds=[100, 120, 140, 160, 180, 200, 220, 240, 260, 280,
                   300, 325, 350, 375, 400, 450, 500, 600, 750, 1000],
        icon="bolt",
    ))

    # Average WPM (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="speed_avg",
        name_template="Average Speed {tier}",
        description_template="Maintain an average WPM of {value}+",
        category=AchievementCategory.SPEED,
        metric_type="average_wpm",
        thresholds=[50, 60, 70, 80, 90, 100, 110, 120, 130, 140,
                   150, 160, 175, 190, 210, 230, 250, 280, 320, 400],
        icon="gauge",
    ))

    # High-Speed Transcription Count (20 tiers) - transcriptions over 150 WPM
    achievements.extend(generate_tiered_achievements(
        id_prefix="speed_high_count",
        name_template="High Speed Count {tier}",
        description_template="Complete {value} transcriptions at 150+ WPM",
        category=AchievementCategory.SPEED,
        metric_type="high_speed_count",
        thresholds=[5, 10, 25, 50, 100, 200, 350, 500, 750, 1000,
                   1500, 2000, 3000, 5000, 7500, 10000, 15000, 25000, 50000, 100000],
        icon="fast-forward",
    ))

    # Ultra-Speed Count (20 tiers) - transcriptions over 200 WPM
    achievements.extend(generate_tiered_achievements(
        id_prefix="speed_ultra_count",
        name_template="Ultra Speed {tier}",
        description_template="Complete {value} transcriptions at 200+ WPM",
        category=AchievementCategory.SPEED,
        metric_type="ultra_speed_count",
        thresholds=[1, 5, 10, 25, 50, 100, 200, 350, 500, 750,
                   1000, 1500, 2500, 4000, 6000, 8000, 12000, 20000, 35000, 60000],
        icon="rocket",
    ))

    # =============================================================================
    # CONTEXT MASTERY ACHIEVEMENTS (160 total)
    # 8 contexts × 20 tiers each
    # =============================================================================

    contexts = [
        ("email", "Email Specialist", "mail", "email"),
        ("slack", "Chat Specialist", "chat", "slack"),
        ("meeting", "Meeting Specialist", "users", "meeting_notes"),
        ("document", "Document Specialist", "file-text", "document"),
        ("code", "Code Specialist", "code", "code_comments"),
        ("social", "Social Specialist", "share", "social_media"),
        ("creative", "Creative Specialist", "pen", "creative"),
        ("general", "General Specialist", "grid", "general"),
    ]

    for ctx_key, ctx_name, ctx_icon, metric_suffix in contexts:
        # Context usage count (10 tiers)
        achievements.extend(generate_tiered_achievements(
            id_prefix=f"ctx_{ctx_key}_count",
            name_template=f"{ctx_name} {{tier}}",
            description_template=f"Complete {{value}} {ctx_key} transcriptions",
            category=AchievementCategory.CONTEXT,
            metric_type=f"context_{metric_suffix}_count",
            thresholds=[5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000],
            icon=ctx_icon,
        ))

        # Context word count (10 tiers)
        achievements.extend(generate_tiered_achievements(
            id_prefix=f"ctx_{ctx_key}_words",
            name_template=f"{ctx_name} Words {{tier}}",
            description_template=f"Transcribe {{threshold}} words in {ctx_key} context",
            category=AchievementCategory.CONTEXT,
            metric_type=f"context_{metric_suffix}_words",
            thresholds=[500, 1000, 2500, 5000, 10000, 25000, 50000, 100000, 250000, 500000],
            icon=ctx_icon,
        ))

    # =============================================================================
    # FORMALITY ACHIEVEMENTS (60 total)
    # 3 formality levels × 20 tiers each
    # =============================================================================

    formality_levels = [
        ("casual", "Casual Style", "message-circle"),
        ("neutral", "Neutral Style", "minus"),
        ("formal", "Formal Style", "briefcase"),
    ]

    for form_key, form_name, form_icon in formality_levels:
        # Formality usage count (10 tiers)
        achievements.extend(generate_tiered_achievements(
            id_prefix=f"form_{form_key}_count",
            name_template=f"{form_name} {{tier}}",
            description_template=f"Complete {{value}} transcriptions in {form_key} formality",
            category=AchievementCategory.FORMALITY,
            metric_type=f"formality_{form_key}_count",
            thresholds=[10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000],
            icon=form_icon,
        ))

        # Formality word count (10 tiers)
        achievements.extend(generate_tiered_achievements(
            id_prefix=f"form_{form_key}_words",
            name_template=f"{form_name} Words {{tier}}",
            description_template=f"Transcribe {{threshold}} words in {form_key} formality",
            category=AchievementCategory.FORMALITY,
            metric_type=f"formality_{form_key}_words",
            thresholds=[1000, 2500, 5000, 10000, 25000, 50000, 100000, 250000, 500000, 1000000],
            icon=form_icon,
        ))

    # =============================================================================
    # AI TRAINING/LEARNING ACHIEVEMENTS (120 total)
    # =============================================================================

    # Total Corrections (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="learn_corrections",
        name_template="Corrections {tier}",
        description_template="Submit {value} corrections to improve AI",
        category=AchievementCategory.LEARNING,
        metric_type="total_corrections",
        thresholds=[5, 10, 25, 50, 100, 200, 350, 500, 750, 1000,
                   1500, 2500, 4000, 6000, 8000, 10000, 15000, 25000, 40000, 75000],
        icon="edit",
    ))

    # Spelling Corrections (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="learn_spelling",
        name_template="Spelling Fixes {tier}",
        description_template="Submit {value} spelling corrections",
        category=AchievementCategory.LEARNING,
        metric_type="spelling_corrections",
        thresholds=[5, 10, 25, 50, 100, 200, 350, 500, 750, 1000,
                   1500, 2000, 3000, 4500, 6000, 8000, 12000, 18000, 30000, 50000],
        icon="spell-check",
    ))

    # Grammar Corrections (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="learn_grammar",
        name_template="Grammar Fixes {tier}",
        description_template="Submit {value} grammar corrections",
        category=AchievementCategory.LEARNING,
        metric_type="grammar_corrections",
        thresholds=[5, 10, 25, 50, 100, 200, 350, 500, 750, 1000,
                   1500, 2000, 3000, 4500, 6000, 8000, 12000, 18000, 30000, 50000],
        icon="check-circle",
    ))

    # Audio Samples (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="learn_audio",
        name_template="Audio Samples {tier}",
        description_template="Contribute {value} audio samples for training",
        category=AchievementCategory.LEARNING,
        metric_type="audio_samples",
        thresholds=[1, 5, 10, 25, 50, 100, 200, 350, 500, 750,
                   1000, 1500, 2500, 4000, 6000, 8500, 12000, 18000, 30000, 50000],
        icon="headphones",
    ))

    # Custom Dictionary Entries (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="learn_dictionary",
        name_template="Dictionary Entries {tier}",
        description_template="Add {value} custom dictionary entries",
        category=AchievementCategory.LEARNING,
        metric_type="dictionary_entries",
        thresholds=[5, 10, 25, 50, 100, 200, 350, 500, 750, 1000,
                   1500, 2000, 3000, 5000, 7500, 10000, 15000, 25000, 40000, 75000],
        icon="book",
    ))

    # Correction Rules Created (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="learn_rules",
        name_template="Custom Rules {tier}",
        description_template="Create {value} custom correction rules",
        category=AchievementCategory.LEARNING,
        metric_type="correction_rules",
        thresholds=[1, 3, 5, 10, 20, 35, 50, 75, 100, 150,
                   200, 300, 500, 750, 1000, 1500, 2500, 4000, 6500, 10000],
        icon="settings",
    ))

    # =============================================================================
    # TEMPORAL/BEHAVIORAL ACHIEVEMENTS (120 total)
    # =============================================================================

    # Hour of Day achievements (24 hours × 3 tiers = 72)
    hour_icons = {
        range(5, 9): "sunrise",
        range(9, 12): "sun",
        range(12, 18): "sun",
        range(18, 22): "sunset",
    }

    for hour in range(24):
        hour_12 = hour % 12 or 12
        am_pm = "AM" if hour < 12 else "PM"
        hour_name = f"{hour_12}{am_pm}"

        icon = "moon"
        for hr_range, hr_icon in hour_icons.items():
            if hour in hr_range:
                icon = hr_icon
                break

        achievements.extend(generate_tiered_achievements(
            id_prefix=f"time_hour_{hour}",
            name_template=f"{hour_name} User {{tier}}",
            description_template=f"Complete {{value}} transcriptions at {hour_name}",
            category=AchievementCategory.TEMPORAL,
            metric_type=f"hour_{hour}_count",
            thresholds=[10, 50, 200],
            icon=icon,
        ))

    # Day of Week achievements (7 days × 3 tiers = 21)
    days = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]

    for i, day in enumerate(days):
        achievements.extend(generate_tiered_achievements(
            id_prefix=f"time_day_{day.lower()}",
            name_template=f"{day} User {{tier}}",
            description_template=f"Complete {{value}} transcriptions on {day}s",
            category=AchievementCategory.TEMPORAL,
            metric_type=f"day_{i}_count",
            thresholds=[25, 100, 500],
            icon="calendar",
        ))

    # Early Bird (5 tiers) - transcriptions before 7 AM
    achievements.extend(generate_tiered_achievements(
        id_prefix="time_early_bird",
        name_template="Early Bird {tier}",
        description_template="Complete {value} transcriptions before 7 AM",
        category=AchievementCategory.TEMPORAL,
        metric_type="early_bird_count",
        thresholds=[5, 25, 100, 500, 2000],
        icon="sunrise",
    ))

    # Night Owl (5 tiers) - transcriptions after 10 PM
    achievements.extend(generate_tiered_achievements(
        id_prefix="time_night_owl",
        name_template="Night Owl {tier}",
        description_template="Complete {value} transcriptions after 10 PM",
        category=AchievementCategory.TEMPORAL,
        metric_type="night_owl_count",
        thresholds=[5, 25, 100, 500, 2000],
        icon="moon",
    ))

    # Weekend Warrior (5 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="time_weekend",
        name_template="Weekend User {tier}",
        description_template="Complete {value} transcriptions on weekends",
        category=AchievementCategory.TEMPORAL,
        metric_type="weekend_count",
        thresholds=[10, 50, 200, 1000, 5000],
        icon="coffee",
    ))

    # Workweek Hero (5 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="time_workweek",
        name_template="Weekday User {tier}",
        description_template="Complete {value} transcriptions Mon-Fri",
        category=AchievementCategory.TEMPORAL,
        metric_type="workweek_count",
        thresholds=[25, 100, 500, 2500, 10000],
        icon="briefcase",
    ))

    # =============================================================================
    # RECORDS/MILESTONES ACHIEVEMENTS (80 total)
    # =============================================================================

    # Longest Single Transcription (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="rec_longest",
        name_template="Long Form {tier}",
        description_template="Complete a single transcription with {value}+ words",
        category=AchievementCategory.RECORDS,
        metric_type="longest_transcription",
        thresholds=[100, 200, 300, 500, 750, 1000, 1500, 2000, 3000, 4000,
                   5000, 7500, 10000, 15000, 20000, 30000, 50000, 75000, 100000, 150000],
        icon="file-text",
    ))

    # Time Saved (20 tiers) - in minutes
    achievements.extend(generate_tiered_achievements(
        id_prefix="rec_time_saved",
        name_template="Time Saved {tier}",
        description_template="Save {value} minutes of typing time",
        category=AchievementCategory.RECORDS,
        metric_type="time_saved_minutes",
        thresholds=[30, 60, 120, 300, 600, 1200, 2400, 4800, 7200, 10800,
                   18000, 30000, 45000, 72000, 108000, 180000, 360000, 720000, 1440000, 3000000],
        icon="clock",
    ))

    # Months as User (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="rec_tenure",
        name_template="Member {tier}",
        description_template="Be a member for {value} months",
        category=AchievementCategory.RECORDS,
        metric_type="account_age_months",
        thresholds=[1, 2, 3, 6, 9, 12, 18, 24, 30, 36,
                   42, 48, 60, 72, 84, 96, 108, 120, 144, 180],
        icon="award",
    ))

    # Most Productive Day Record (20 tiers)
    achievements.extend(generate_tiered_achievements(
        id_prefix="rec_productive_day",
        name_template="Personal Best {tier}",
        description_template="Have {value} personal best productive days",
        category=AchievementCategory.RECORDS,
        metric_type="productive_day_records",
        thresholds=[1, 3, 5, 10, 15, 25, 40, 60, 80, 100,
                   150, 200, 300, 450, 600, 800, 1000, 1500, 2000, 3000],
        icon="trending-up",
    ))

    # =============================================================================
    # COMBINATION ACHIEVEMENTS (100 total)
    # =============================================================================

    # Speed + Volume (20 tiers) - high WPM with high word count
    achievements.extend(generate_tiered_achievements(
        id_prefix="combo_speed_vol",
        name_template="Speed & Volume {tier}",
        description_template="Complete {value} transcriptions with 150+ WPM and 100+ words",
        category=AchievementCategory.COMBO,
        metric_type="combo_speed_volume",
        thresholds=[1, 5, 10, 25, 50, 100, 200, 350, 500, 750,
                   1000, 1500, 2500, 4000, 6000, 8500, 12000, 18000, 30000, 50000],
        icon="zap",
    ))

    # Streak + Volume (20 tiers) - maintain streak with daily minimum
    achievements.extend(generate_tiered_achievements(
        id_prefix="combo_streak_vol",
        name_template="Consistent Output {tier}",
        description_template="Maintain a {value}-day streak with 500+ words daily",
        category=AchievementCategory.COMBO,
        metric_type="combo_streak_volume",
        thresholds=[3, 5, 7, 14, 21, 30, 45, 60, 90, 120,
                   150, 180, 250, 365, 500, 730, 1000, 1500, 2000, 2500],
        icon="activity",
    ))

    # Context Diversity (20 tiers) - use multiple contexts
    achievements.extend(generate_tiered_achievements(
        id_prefix="combo_diversity",
        name_template="Context Variety {tier}",
        description_template="Use {value} different contexts with 100+ transcriptions each",
        category=AchievementCategory.COMBO,
        metric_type="combo_context_diversity",
        thresholds=[2, 3, 4, 5, 6, 7, 8, 8, 8, 8,
                   8, 8, 8, 8, 8, 8, 8, 8, 8, 8],  # Max 8 contexts
        icon="grid",
    ))

    # Multi-Metric Excellence (20 tiers) - excel across multiple categories
    achievements.extend(generate_tiered_achievements(
        id_prefix="combo_excellence",
        name_template="Multi-Category {tier}",
        description_template="Achieve tier {value}+ in {value} different achievement categories",
        category=AchievementCategory.COMBO,
        metric_type="combo_multi_excellence",
        thresholds=[1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
                   10, 10, 10, 10, 10, 10, 10, 10, 10, 10],  # Max 10 categories
        icon="star",
    ))

    # Daily + Speed (20 tiers) - fast transcriptions in a day
    achievements.extend(generate_tiered_achievements(
        id_prefix="combo_daily_speed",
        name_template="Daily Speed {tier}",
        description_template="Complete {value} transcriptions at 150+ WPM in a single day",
        category=AchievementCategory.COMBO,
        metric_type="combo_daily_speed",
        thresholds=[3, 5, 10, 15, 25, 40, 60, 85, 120, 160,
                   200, 250, 320, 400, 500, 650, 850, 1100, 1500, 2000],
        icon="target",
    ))

    # =============================================================================
    # SPECIAL/HIDDEN ACHIEVEMENTS (60 total)
    # =============================================================================

    # Fibonacci transcription count
    achievements.append({
        "id": "special_fibonacci",
        "name": "Fibonacci Sequence",
        "description": "Complete exactly 1, 1, 2, 3, 5, 8, 13, 21, 34, or 55 transcriptions on the same day",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.RARE,
        "xp_reward": 100,
        "icon": "hash",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_fibonacci",
        "is_hidden": True,
        "parent_id": None,
    })

    # Palindrome word count
    achievements.append({
        "id": "special_palindrome",
        "name": "Palindrome Count",
        "description": "Complete a transcription with a palindrome word count (121, 1221, etc.)",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.RARE,
        "xp_reward": 75,
        "icon": "rotate-cw",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_palindrome",
        "is_hidden": True,
        "parent_id": None,
    })

    # Prime time - transcription at XX:XX where both are prime
    achievements.append({
        "id": "special_prime_time",
        "name": "Prime Time",
        "description": "Complete a transcription at a time with prime hours and minutes (e.g., 11:13, 5:07)",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.COMMON,
        "xp_reward": 25,
        "icon": "hash",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_prime_time",
        "is_hidden": True,
        "parent_id": None,
    })

    # Achievement Hunter (10 tiers) - total achievements unlocked
    achievements.extend(generate_tiered_achievements(
        id_prefix="special_collector",
        name_template="Achievement Collector {tier}",
        description_template="Unlock {value} total achievements",
        category=AchievementCategory.SPECIAL,
        metric_type="total_achievements",
        thresholds=[10, 25, 50, 100, 200, 350, 500, 750, 1000, 1100],
        icon="award",
    ))

    # Rarity Collector achievements
    achievements.extend(generate_tiered_achievements(
        id_prefix="special_common",
        name_template="Common Collector {tier}",
        description_template="Unlock {value} Common achievements",
        category=AchievementCategory.SPECIAL,
        metric_type="common_achievements",
        thresholds=[10, 25, 50, 100, 200, 300, 400, 500, 550],
        icon="circle",
    ))

    achievements.extend(generate_tiered_achievements(
        id_prefix="special_rare",
        name_template="Rare Collector {tier}",
        description_template="Unlock {value} Rare achievements",
        category=AchievementCategory.SPECIAL,
        metric_type="rare_achievements",
        thresholds=[5, 15, 30, 60, 100, 150, 200, 275, 330],
        icon="square",
    ))

    achievements.extend(generate_tiered_achievements(
        id_prefix="special_epic",
        name_template="Epic Collector {tier}",
        description_template="Unlock {value} Epic achievements",
        category=AchievementCategory.SPECIAL,
        metric_type="epic_achievements",
        thresholds=[3, 8, 15, 30, 50, 80, 110, 140, 165],
        icon="hexagon",
    ))

    achievements.extend(generate_tiered_achievements(
        id_prefix="special_legendary",
        name_template="Legendary Collector {tier}",
        description_template="Unlock {value} Legendary achievements",
        category=AchievementCategory.SPECIAL,
        metric_type="legendary_achievements",
        thresholds=[1, 3, 7, 12, 20, 30, 40, 50, 55],
        icon="diamond",
    ))

    # Category Mastery - complete all achievements in a category
    category_counts = {
        "volume": 200, "streak": 120, "speed": 80, "context": 160,
        "formality": 60, "learning": 120, "temporal": 120, "records": 80,
        "combo": 100
    }

    for cat, count in category_counts.items():
        achievements.append({
            "id": f"special_master_{cat}",
            "name": f"{cat.title()} Master",
            "description": f"Unlock all {count} achievements in the {cat.title()} category",
            "category": AchievementCategory.SPECIAL,
            "rarity": AchievementRarity.LEGENDARY,
            "xp_reward": 2500,
            "icon": "crown",
            "tier": 1,
            "threshold": count,
            "metric_type": f"category_{cat}_complete",
            "is_hidden": False,
            "parent_id": None,
        })

    # First transcription
    achievements.append({
        "id": "special_first",
        "name": "First Transcription",
        "description": "Complete your very first transcription",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.COMMON,
        "xp_reward": 50,
        "icon": "play",
        "tier": 1,
        "threshold": 1,
        "metric_type": "total_transcriptions",
        "is_hidden": False,
        "parent_id": None,
    })

    # New Year's transcription
    achievements.append({
        "id": "special_new_year",
        "name": "New Year Transcription",
        "description": "Complete a transcription on January 1st",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.RARE,
        "xp_reward": 150,
        "icon": "calendar",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_new_year",
        "is_hidden": True,
        "parent_id": None,
    })

    # Midnight transcription
    achievements.append({
        "id": "special_midnight",
        "name": "Midnight Session",
        "description": "Complete a transcription exactly at midnight (00:00)",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.EPIC,
        "xp_reward": 200,
        "icon": "moon",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_midnight",
        "is_hidden": True,
        "parent_id": None,
    })

    # 100 words exactly
    achievements.append({
        "id": "special_100_words",
        "name": "Century Mark",
        "description": "Complete a transcription with exactly 100 words",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.RARE,
        "xp_reward": 100,
        "icon": "target",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_100_words",
        "is_hidden": True,
        "parent_id": None,
    })

    # Weekend Supreme
    achievements.append({
        "id": "special_weekend_supreme",
        "name": "Weekend Surge",
        "description": "Transcribe more on a single weekend than entire previous week",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.EPIC,
        "xp_reward": 300,
        "icon": "trending-up",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_weekend_supreme",
        "is_hidden": True,
        "parent_id": None,
    })

    # Birthday transcription
    achievements.append({
        "id": "special_birthday",
        "name": "Birthday Session",
        "description": "Complete a transcription on your birthday",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.RARE,
        "xp_reward": 200,
        "icon": "gift",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_birthday",
        "is_hidden": True,
        "parent_id": None,
    })

    # Halloween transcription
    achievements.append({
        "id": "special_halloween",
        "name": "Halloween Session",
        "description": "Complete a transcription on Halloween (October 31st)",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.RARE,
        "xp_reward": 150,
        "icon": "moon",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_halloween",
        "is_hidden": True,
        "parent_id": None,
    })

    # Valentine's Day transcription
    achievements.append({
        "id": "special_valentine",
        "name": "Valentine Session",
        "description": "Complete a transcription on Valentine's Day (February 14th)",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.RARE,
        "xp_reward": 150,
        "icon": "heart",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_valentine",
        "is_hidden": True,
        "parent_id": None,
    })

    # Complete all prestige tiers
    achievements.append({
        "id": "special_all_tiers",
        "name": "Legend Status",
        "description": "Reach the Legend prestige tier",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.LEGENDARY,
        "xp_reward": 10000,
        "icon": "crown",
        "tier": 1,
        "threshold": 1,
        "metric_type": "reached_legend_tier",
        "is_hidden": False,
        "parent_id": None,
    })

    # 1000 words in one transcription
    achievements.append({
        "id": "special_thousand_words",
        "name": "Thousand Words",
        "description": "Complete a single transcription with exactly 1,000 words",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.EPIC,
        "xp_reward": 250,
        "icon": "target",
        "tier": 1,
        "threshold": 1,
        "metric_type": "special_thousand_words",
        "is_hidden": True,
        "parent_id": None,
    })

    # Use all 8 contexts in one day
    achievements.append({
        "id": "special_context_rainbow",
        "name": "All Contexts",
        "description": "Use all 8 context types in a single day",
        "category": AchievementCategory.SPECIAL,
        "rarity": AchievementRarity.EPIC,
        "xp_reward": 350,
        "icon": "grid",
        "tier": 1,
        "threshold": 8,
        "metric_type": "daily_context_variety",
        "is_hidden": True,
        "parent_id": None,
    })

    return achievements


def get_achievement_count() -> int:
    """Return the total number of achievements generated."""
    return len(generate_all_achievements())
