const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();

/** Time today, weekday this week, otherwise a date. */
export function shortDate(timestamp: number): string {
	const date = new Date(timestamp * 1000);
	const now = new Date();
	if (sameDay(date, now)) {
		return date.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
	}
	const days = (now.getTime() - date.getTime()) / 86_400_000;
	if (days < 6) return date.toLocaleDateString(undefined, { weekday: 'short' });
	const sameYear = date.getFullYear() === now.getFullYear();
	return date.toLocaleDateString(undefined, {
		day: 'numeric',
		month: 'short',
		year: sameYear ? undefined : 'numeric'
	});
}

export function fullDate(timestamp: number): string {
	return new Date(timestamp * 1000).toLocaleString(undefined, {
		weekday: 'short',
		day: 'numeric',
		month: 'short',
		year: 'numeric',
		hour: 'numeric',
		minute: '2-digit'
	});
}

export function fileSize(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
	return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function displayName(name: string | null | undefined, address: string | null | undefined): string {
	return name?.trim() || address || 'Unknown sender';
}

export function initials(name: string): string {
	const words = name.replace(/[^\p{L}\p{N}@. ]/gu, '').split(/[\s.@]+/).filter(Boolean);
	return ((words[0]?.[0] ?? '?') + (words[1]?.[0] ?? '')).toUpperCase();
}

/** Stable hue per sender so the same person always gets the same avatar colour. */
export function hue(seed: string): number {
	let hash = 0;
	for (const char of seed) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
	return hash % 360;
}
