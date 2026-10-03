interface ProgressProps {
  value: number;
  label: string;
}

export function Progress({ value, label }: ProgressProps) {
  const bounded = Math.min(100, Math.max(0, value));
  return (
    <div className="progress" role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={bounded}>
      <div className="progress__fill" style={{ width: `${bounded}%` }} />
    </div>
  );
}
