import { useMemo, useState } from 'react';

interface DayData {
  date: string;
  words: number;
  count: number;
}

interface HeatMapProps {
  data: DayData[];
  weeks?: number;
}

/**
 * GitHub-style contribution heatmap showing daily activity
 */
export function HeatMap({ data, weeks = 12 }: HeatMapProps) {
  const [hoveredCell, setHoveredCell] = useState<{
    date: string;
    words: number;
    count: number;
    x: number;
    y: number;
  } | null>(null);

  // Generate grid data for the last N weeks
  const gridData = useMemo(() => {
    const today = new Date();
    const grid: (DayData | null)[][] = [];

    // Create a map for quick lookup
    const dataMap = new Map<string, DayData>();
    data.forEach(d => {
      dataMap.set(d.date, d);
    });

    // Calculate the start date (N weeks ago, aligned to Monday)
    const startDate = new Date(today);
    startDate.setDate(startDate.getDate() - (weeks * 7) - startDate.getDay() + 1);

    // Build the grid (columns = weeks, rows = days of week)
    let currentDate = new Date(startDate);

    while (currentDate <= today) {
      const weekColumn: (DayData | null)[] = [];

      for (let day = 0; day < 7; day++) {
        if (currentDate <= today) {
          const dateStr = currentDate.toISOString().split('T')[0];
          const dayData = dataMap.get(dateStr);

          weekColumn.push(dayData || { date: dateStr, words: 0, count: 0 });
        } else {
          weekColumn.push(null);
        }
        currentDate.setDate(currentDate.getDate() + 1);
      }

      grid.push(weekColumn);
    }

    return grid;
  }, [data, weeks]);

  // Calculate intensity levels
  const maxWords = useMemo(() => {
    const values = data.map(d => d.words).filter(w => w > 0);
    if (values.length === 0) return 1000;
    return Math.max(...values);
  }, [data]);

  const getIntensity = (words: number): number => {
    if (words === 0) return 0;
    const ratio = words / maxWords;
    if (ratio < 0.1) return 1;
    if (ratio < 0.25) return 2;
    if (ratio < 0.5) return 3;
    if (ratio < 0.75) return 4;
    return 5;
  };

  // Generate month labels
  const monthLabels = useMemo(() => {
    const labels: { month: string; position: number }[] = [];
    let lastMonth = -1;

    gridData.forEach((week, weekIndex) => {
      const firstDay = week.find(d => d !== null);
      if (firstDay) {
        const date = new Date(firstDay.date);
        const month = date.getMonth();
        if (month !== lastMonth) {
          labels.push({
            month: date.toLocaleDateString('en-US', { month: 'short' }),
            position: weekIndex,
          });
          lastMonth = month;
        }
      }
    });

    return labels;
  }, [gridData]);

  const dayLabels = ['Mon', '', 'Wed', '', 'Fri', '', 'Sun'];

  return (
    <div className="heatmap-container">
      <div className="heatmap-wrapper">
        {/* Day labels */}
        <div className="heatmap-day-labels">
          {dayLabels.map((label, i) => (
            <div key={i} className="heatmap-day-label">
              {label}
            </div>
          ))}
        </div>

        <div className="heatmap-main">
          {/* Month labels */}
          <div className="heatmap-month-labels">
            {monthLabels.map((label, i) => (
              <div
                key={i}
                className="heatmap-month-label"
                style={{ gridColumn: label.position + 1 }}
              >
                {label.month}
              </div>
            ))}
          </div>

          {/* Grid */}
          <div
            className="heatmap-grid"
            style={{ gridTemplateColumns: `repeat(${gridData.length}, 1fr)` }}
          >
            {gridData.map((week, weekIndex) => (
              <div key={weekIndex} className="heatmap-week">
                {week.map((day, dayIndex) => {
                  if (!day) {
                    return <div key={dayIndex} className="heatmap-cell empty" />;
                  }

                  const intensity = getIntensity(day.words);

                  return (
                    <div
                      key={dayIndex}
                      className={`heatmap-cell intensity-${intensity}`}
                      onMouseEnter={(e) => {
                        const rect = e.currentTarget.getBoundingClientRect();
                        setHoveredCell({
                          ...day,
                          x: rect.left + rect.width / 2,
                          y: rect.top,
                        });
                      }}
                      onMouseLeave={() => setHoveredCell(null)}
                    />
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Legend */}
      <div className="heatmap-legend">
        <span className="legend-label">Less</span>
        <div className="legend-cells">
          {[0, 1, 2, 3, 4, 5].map(i => (
            <div key={i} className={`heatmap-cell intensity-${i}`} />
          ))}
        </div>
        <span className="legend-label">More</span>
      </div>

      {/* Tooltip */}
      {hoveredCell && (
        <div
          className="heatmap-tooltip"
          style={{
            left: hoveredCell.x,
            top: hoveredCell.y - 10,
          }}
        >
          <div className="tooltip-date">
            {new Date(hoveredCell.date).toLocaleDateString('en-US', {
              weekday: 'short',
              month: 'short',
              day: 'numeric',
            })}
          </div>
          <div className="tooltip-stats">
            {hoveredCell.words > 0 ? (
              <>
                <span className="tooltip-words">{hoveredCell.words.toLocaleString()} words</span>
                <span className="tooltip-count">{hoveredCell.count} transcription{hoveredCell.count !== 1 ? 's' : ''}</span>
              </>
            ) : (
              <span className="tooltip-empty">No activity</span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/**
 * Compact heatmap for smaller displays
 */
export function MiniHeatMap({ data }: { data: DayData[] }) {
  return <HeatMap data={data} weeks={8} />;
}
