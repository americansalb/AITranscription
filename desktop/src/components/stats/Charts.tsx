import { useMemo } from 'react';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  Cell,
  AreaChart,
  Area,
  PieChart,
  Pie,
} from 'recharts';

const ACCENT_COLOR = '#6366f1';
const ACCENT_HOVER = '#818cf8';
const ACCENT_LIGHT = 'rgba(99, 102, 241, 0.2)';

// ============================================
// HOURLY ACTIVITY CHART
// Bar chart showing words per hour (0-23)
// ============================================

interface HourlyData {
  hour: number;
  words: number;
  count: number;
}

interface HourlyActivityChartProps {
  data: HourlyData[];
}

function formatHour(hour: number): string {
  if (hour === 0) return '12a';
  if (hour === 12) return '12p';
  return hour < 12 ? `${hour}a` : `${hour - 12}p`;
}

export function HourlyActivityChart({ data }: HourlyActivityChartProps) {
  const processedData = useMemo(() => {
    // Create a full 24-hour array
    const hourlyMap = new Map<number, { words: number; count: number }>();
    data.forEach(d => {
      const existing = hourlyMap.get(d.hour) || { words: 0, count: 0 };
      hourlyMap.set(d.hour, {
        words: existing.words + d.words,
        count: existing.count + d.count,
      });
    });

    return Array.from({ length: 24 }, (_, hour) => ({
      hour,
      label: formatHour(hour),
      words: hourlyMap.get(hour)?.words || 0,
      count: hourlyMap.get(hour)?.count || 0,
    }));
  }, [data]);

  const maxWords = Math.max(...processedData.map(d => d.words));
  const peakHour = processedData.find(d => d.words === maxWords && maxWords > 0);

  const CustomTooltip = ({ active, payload }: any) => {
    if (active && payload && payload.length) {
      const data = payload[0].payload;
      return (
        <div className="chart-tooltip">
          <p className="tooltip-label">{formatHour(data.hour)}</p>
          <p className="tooltip-value">{data.words.toLocaleString()} words</p>
          <p className="tooltip-secondary">{data.count} transcriptions</p>
        </div>
      );
    }
    return null;
  };

  return (
    <div className="chart-container hourly-activity-chart">
      <div className="chart-header">
        <h3 className="chart-title">When Do You Transcribe?</h3>
        {peakHour && (
          <span className="peak-indicator">Peak: {formatHour(peakHour.hour)}</span>
        )}
      </div>
      <ResponsiveContainer width="100%" height={160}>
        <BarChart data={processedData} margin={{ top: 10, right: 10, left: -25, bottom: 0 }}>
          <XAxis
            dataKey="label"
            axisLine={false}
            tickLine={false}
            tick={{ fill: '#71717a', fontSize: 9 }}
            interval={2}
          />
          <YAxis
            axisLine={false}
            tickLine={false}
            tick={{ fill: '#71717a', fontSize: 9 }}
            tickFormatter={(v) => v >= 1000 ? `${(v / 1000).toFixed(0)}k` : v}
          />
          <Tooltip content={<CustomTooltip />} cursor={{ fill: ACCENT_LIGHT }} />
          <Bar dataKey="words" radius={[2, 2, 0, 0]}>
            {processedData.map((entry, index) => (
              <Cell
                key={`cell-${index}`}
                fill={entry.words === maxWords && maxWords > 0 ? ACCENT_HOVER : ACCENT_COLOR}
              />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}

// ============================================
// DAY OF WEEK CHART
// Horizontal bar chart showing Mon-Sun breakdown
// ============================================

const DAYS_FULL = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const DAYS_SHORT = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

interface DayOfWeekData {
  day: number; // 0-6 (Sun-Sat)
  words: number;
  count: number;
}

interface DayOfWeekChartProps {
  data: DayOfWeekData[];
}

export function DayOfWeekChart({ data }: DayOfWeekChartProps) {
  const processedData = useMemo(() => {
    const dayMap = new Map<number, { words: number; count: number }>();
    data.forEach(d => {
      const existing = dayMap.get(d.day) || { words: 0, count: 0 };
      dayMap.set(d.day, {
        words: existing.words + d.words,
        count: existing.count + d.count,
      });
    });

    return DAYS_SHORT.map((name, index) => ({
      day: index,
      name,
      fullName: DAYS_FULL[index],
      words: dayMap.get(index)?.words || 0,
      count: dayMap.get(index)?.count || 0,
    }));
  }, [data]);

  const maxWords = Math.max(...processedData.map(d => d.words));
  const peakDay = processedData.find(d => d.words === maxWords && maxWords > 0);

  const CustomTooltip = ({ active, payload }: any) => {
    if (active && payload && payload.length) {
      const data = payload[0].payload;
      return (
        <div className="chart-tooltip">
          <p className="tooltip-label">{data.fullName}</p>
          <p className="tooltip-value">{data.words.toLocaleString()} words</p>
          <p className="tooltip-secondary">{data.count} transcriptions</p>
        </div>
      );
    }
    return null;
  };

  return (
    <div className="chart-container day-of-week-chart">
      <div className="chart-header">
        <h3 className="chart-title">Your Weekly Rhythm</h3>
        {peakDay && (
          <span className="peak-indicator">Peak: {peakDay.name}</span>
        )}
      </div>
      <ResponsiveContainer width="100%" height={180}>
        <BarChart
          data={processedData}
          layout="vertical"
          margin={{ top: 5, right: 20, left: 5, bottom: 5 }}
        >
          <XAxis
            type="number"
            axisLine={false}
            tickLine={false}
            tick={{ fill: '#71717a', fontSize: 9 }}
            tickFormatter={(v) => v >= 1000 ? `${(v / 1000).toFixed(0)}k` : v}
          />
          <YAxis
            type="category"
            dataKey="name"
            axisLine={false}
            tickLine={false}
            tick={{ fill: '#71717a', fontSize: 11 }}
            width={35}
          />
          <Tooltip content={<CustomTooltip />} cursor={{ fill: ACCENT_LIGHT }} />
          <Bar dataKey="words" radius={[0, 4, 4, 0]}>
            {processedData.map((entry, index) => (
              <Cell
                key={`cell-${index}`}
                fill={entry.words === maxWords && maxWords > 0 ? ACCENT_HOVER : ACCENT_COLOR}
              />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}

// ============================================
// MONTHLY TREND CHART
// Area chart showing growth over time
// ============================================

interface MonthlyData {
  month: string; // e.g., "Jan", "Feb"
  words: number;
  transcriptions: number;
}

interface MonthlyTrendChartProps {
  data: MonthlyData[];
}

export function MonthlyTrendChart({ data }: MonthlyTrendChartProps) {
  const CustomTooltip = ({ active, payload, label }: any) => {
    if (active && payload && payload.length) {
      return (
        <div className="chart-tooltip">
          <p className="tooltip-label">{label}</p>
          <p className="tooltip-value">{payload[0].value.toLocaleString()} words</p>
          <p className="tooltip-secondary">{payload[0].payload.transcriptions} transcriptions</p>
        </div>
      );
    }
    return null;
  };

  // Calculate trend direction
  const trendDirection = useMemo(() => {
    if (data.length < 2) return 'neutral';
    const firstHalf = data.slice(0, Math.floor(data.length / 2));
    const secondHalf = data.slice(Math.floor(data.length / 2));
    const firstAvg = firstHalf.reduce((s, d) => s + d.words, 0) / firstHalf.length;
    const secondAvg = secondHalf.reduce((s, d) => s + d.words, 0) / secondHalf.length;
    if (secondAvg > firstAvg * 1.1) return 'up';
    if (secondAvg < firstAvg * 0.9) return 'down';
    return 'neutral';
  }, [data]);

  return (
    <div className="chart-container monthly-trend-chart">
      <div className="chart-header">
        <h3 className="chart-title">Growth Over Time</h3>
        {trendDirection !== 'neutral' && (
          <span className={`trend-badge ${trendDirection === 'up' ? 'positive' : 'negative'}`}>
            {trendDirection === 'up' ? 'Trending Up' : 'Trending Down'}
          </span>
        )}
      </div>
      <ResponsiveContainer width="100%" height={160}>
        <AreaChart data={data} margin={{ top: 10, right: 10, left: -25, bottom: 0 }}>
          <defs>
            <linearGradient id="monthlyGradient" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor={ACCENT_COLOR} stopOpacity={0.3} />
              <stop offset="95%" stopColor={ACCENT_COLOR} stopOpacity={0} />
            </linearGradient>
          </defs>
          <XAxis
            dataKey="month"
            axisLine={false}
            tickLine={false}
            tick={{ fill: '#71717a', fontSize: 10 }}
          />
          <YAxis
            axisLine={false}
            tickLine={false}
            tick={{ fill: '#71717a', fontSize: 9 }}
            tickFormatter={(v) => v >= 1000 ? `${(v / 1000).toFixed(0)}k` : v}
          />
          <Tooltip content={<CustomTooltip />} />
          <Area
            type="monotone"
            dataKey="words"
            stroke={ACCENT_COLOR}
            strokeWidth={2}
            fill="url(#monthlyGradient)"
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}

// ============================================
// CONTEXT DONUT CHART
// Donut chart showing context breakdown
// ============================================

const CONTEXT_COLORS = [
  '#6366f1', // indigo
  '#8b5cf6', // violet
  '#a855f7', // purple
  '#d946ef', // fuchsia
  '#ec4899', // pink
  '#f43f5e', // rose
];

interface ContextData {
  context: string;
  count: number;
  words: number;
}

interface ContextDonutChartProps {
  data: ContextData[];
}

export function ContextDonutChart({ data }: ContextDonutChartProps) {
  const processedData = useMemo(() => {
    const sorted = [...data].sort((a, b) => b.words - a.words);
    const total = sorted.reduce((s, d) => s + d.words, 0);
    return sorted.map((d, i) => ({
      ...d,
      color: CONTEXT_COLORS[i % CONTEXT_COLORS.length],
      percent: total > 0 ? Math.round((d.words / total) * 100) : 0,
    }));
  }, [data]);

  const CustomTooltip = ({ active, payload }: any) => {
    if (active && payload && payload.length) {
      const data = payload[0].payload;
      return (
        <div className="chart-tooltip">
          <p className="tooltip-label">{data.context}</p>
          <p className="tooltip-value">{data.words.toLocaleString()} words</p>
          <p className="tooltip-secondary">{data.percent}% of total</p>
        </div>
      );
    }
    return null;
  };

  if (data.length === 0) {
    return (
      <div className="chart-container context-donut-chart">
        <h3 className="chart-title">What Do You Talk About?</h3>
        <div className="chart-empty">No context data yet</div>
      </div>
    );
  }

  return (
    <div className="chart-container context-donut-chart">
      <h3 className="chart-title">What Do You Talk About?</h3>
      <div className="donut-chart-wrapper">
        <ResponsiveContainer width="100%" height={140}>
          <PieChart>
            <Pie
              data={processedData}
              cx="50%"
              cy="50%"
              innerRadius={35}
              outerRadius={55}
              paddingAngle={2}
              dataKey="words"
            >
              {processedData.map((entry, index) => (
                <Cell key={`cell-${index}`} fill={entry.color} />
              ))}
            </Pie>
            <Tooltip content={<CustomTooltip />} />
          </PieChart>
        </ResponsiveContainer>
        <div className="donut-legend">
          {processedData.slice(0, 4).map((item, i) => (
            <div key={i} className="donut-legend-item">
              <span className="legend-dot" style={{ backgroundColor: item.color }} />
              <span className="legend-name">{item.context}</span>
              <span className="legend-percent">{item.percent}%</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

// ============================================
// WORD LENGTH DISTRIBUTION CHART
// Bar chart showing transcription length distribution
// ============================================

interface WordLengthData {
  range: string;
  count: number;
}

interface WordLengthChartProps {
  data: WordLengthData[];
}

export function WordLengthChart({ data }: WordLengthChartProps) {
  const maxCount = Math.max(...data.map(d => d.count));

  const CustomTooltip = ({ active, payload }: any) => {
    if (active && payload && payload.length) {
      const data = payload[0].payload;
      return (
        <div className="chart-tooltip">
          <p className="tooltip-label">{data.range} words</p>
          <p className="tooltip-value">{data.count} transcriptions</p>
        </div>
      );
    }
    return null;
  };

  return (
    <div className="chart-container word-length-chart">
      <h3 className="chart-title">Transcription Lengths</h3>
      <ResponsiveContainer width="100%" height={140}>
        <BarChart data={data} margin={{ top: 10, right: 10, left: -25, bottom: 0 }}>
          <XAxis
            dataKey="range"
            axisLine={false}
            tickLine={false}
            tick={{ fill: '#71717a', fontSize: 9 }}
          />
          <YAxis
            axisLine={false}
            tickLine={false}
            tick={{ fill: '#71717a', fontSize: 9 }}
          />
          <Tooltip content={<CustomTooltip />} cursor={{ fill: ACCENT_LIGHT }} />
          <Bar dataKey="count" radius={[4, 4, 0, 0]}>
            {data.map((entry, index) => (
              <Cell
                key={`cell-${index}`}
                fill={entry.count === maxCount && maxCount > 0 ? ACCENT_HOVER : ACCENT_COLOR}
              />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}

// ============================================
// HELPER FUNCTIONS
// ============================================

// Aggregate hourly data by hour only (from heatmap data)
export function aggregateByHour(heatmapData: { dayOfWeek: number; hour: number; count: number; words: number }[]): HourlyData[] {
  const byHour = new Map<number, { words: number; count: number }>();

  heatmapData.forEach(d => {
    const existing = byHour.get(d.hour) || { words: 0, count: 0 };
    byHour.set(d.hour, {
      words: existing.words + d.words,
      count: existing.count + d.count,
    });
  });

  return Array.from(byHour.entries()).map(([hour, data]) => ({
    hour,
    words: data.words,
    count: data.count,
  }));
}

// Aggregate heatmap data by day of week
export function aggregateByDayOfWeek(heatmapData: { dayOfWeek: number; hour: number; count: number; words: number }[]): DayOfWeekData[] {
  const byDay = new Map<number, { words: number; count: number }>();

  heatmapData.forEach(d => {
    const existing = byDay.get(d.dayOfWeek) || { words: 0, count: 0 };
    byDay.set(d.dayOfWeek, {
      words: existing.words + d.words,
      count: existing.count + d.count,
    });
  });

  return Array.from(byDay.entries()).map(([day, data]) => ({
    day,
    words: data.words,
    count: data.count,
  }));
}

// Aggregate daily data into monthly data
export function aggregateToMonthly(dailyData: { date: string; words: number; transcriptions: number }[]): MonthlyData[] {
  const byMonth = new Map<string, { words: number; transcriptions: number }>();

  dailyData.forEach(d => {
    const date = new Date(d.date);
    const monthKey = date.toLocaleString('en-US', { month: 'short', year: '2-digit' });
    const existing = byMonth.get(monthKey) || { words: 0, transcriptions: 0 };
    byMonth.set(monthKey, {
      words: existing.words + d.words,
      transcriptions: existing.transcriptions + d.transcriptions,
    });
  });

  // Convert to array and sort by date
  return Array.from(byMonth.entries())
    .map(([month, data]) => ({ month, ...data }))
    .slice(-6); // Last 6 months
}

// Calculate word length distribution from transcripts
export function calculateWordLengthDistribution(transcripts: { word_count: number }[]): WordLengthData[] {
  const ranges = [
    { range: '1-10', min: 1, max: 10 },
    { range: '11-25', min: 11, max: 25 },
    { range: '26-50', min: 26, max: 50 },
    { range: '51-100', min: 51, max: 100 },
    { range: '100+', min: 101, max: Infinity },
  ];

  return ranges.map(({ range, min, max }) => ({
    range,
    count: transcripts.filter(t => t.word_count >= min && t.word_count <= max).length,
  }));
}
