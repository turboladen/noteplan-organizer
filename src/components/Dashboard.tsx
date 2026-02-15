import type { Report } from "../types/api";

interface DashboardProps {
  report: Report;
}

const SEVERITY_STYLES = {
  Info: "bg-blue-50 text-blue-700 border-blue-200",
  Warning: "bg-amber-50 text-amber-700 border-amber-200",
  Error: "bg-red-50 text-red-700 border-red-200",
};

export function Dashboard({ report }: DashboardProps) {
  const { stats } = report;

  return (
    <div className="space-y-6">
      {/* Overview stats */}
      <div className="grid grid-cols-4 gap-4">
        <StatCard label="Notes" value={stats.total_notes} />
        <StatCard label="Daily Notes" value={stats.total_daily_notes} />
        <StatCard label="Weekly Notes" value={stats.total_weekly_notes} />
        <StatCard
          label="Findings"
          value={stats.total_findings}
          highlight={stats.total_findings > 0}
        />
      </div>

      {/* Severity breakdown */}
      <div className="grid grid-cols-3 gap-4">
        {(["Info", "Warning", "Error"] as const).map((sev) => (
          <div
            key={sev}
            className={`rounded-lg border px-4 py-3 ${SEVERITY_STYLES[sev]}`}
          >
            <div className="text-2xl font-bold">
              {stats.findings_by_severity[sev] ?? 0}
            </div>
            <div className="text-sm opacity-80">{sev}</div>
          </div>
        ))}
      </div>

      {/* Category breakdown */}
      <div>
        <h3 className="text-sm font-medium text-gray-500 mb-3">
          Findings by Category
        </h3>
        <div className="grid grid-cols-2 gap-3">
          {Object.entries(stats.findings_by_category)
            .sort(([, a], [, b]) => b - a)
            .map(([category, count]) => (
              <div
                key={category}
                className="flex items-center justify-between bg-white rounded-lg border border-gray-200 px-4 py-2"
              >
                <span className="text-sm text-gray-700">{category}</span>
                <span className="text-sm font-semibold text-gray-900 bg-gray-100 px-2 py-0.5 rounded">
                  {count}
                </span>
              </div>
            ))}
        </div>
      </div>

      {/* Scan info */}
      <div className="text-xs text-gray-400">
        Scanned at {report.scanned_at} &middot; {report.noteplan_path}
      </div>
    </div>
  );
}

function StatCard({
  label,
  value,
  highlight = false,
}: {
  label: string;
  value: number;
  highlight?: boolean;
}) {
  return (
    <div
      className={`rounded-lg border px-4 py-3 ${
        highlight
          ? "bg-amber-50 border-amber-200"
          : "bg-white border-gray-200"
      }`}
    >
      <div
        className={`text-2xl font-bold ${
          highlight ? "text-amber-700" : "text-gray-900"
        }`}
      >
        {value}
      </div>
      <div className="text-sm text-gray-500">{label}</div>
    </div>
  );
}
