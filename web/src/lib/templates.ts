// Filling in a template: its placeholders get the recipient's name and today's date.

/** The placeholders a template may hold, as shown in Settings. */
export const placeholders = ['{first name}', '{last name}', '{name}', '{date}'];

const escape = (text: string) =>
	text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** "Jane Doe" gives "Jane"; "Doe, Jane" gives "Jane". */
export function firstName(name: string): string {
	const trimmed = name.trim();
	if (trimmed.includes(',')) return trimmed.split(',')[1].trim().split(/\s+/)[0] ?? '';
	return trimmed.split(/\s+/)[0] ?? '';
}

/** Words that belong to the surname after them: "Anna van der Berg" gives "van der Berg". */
const particles = new Set(['van', 'von', 'der', 'den', 'de', 'du', 'la', 'le', 'di', 'da', 'del', 'dos', 'ten', 'ter', 'zu', 'zum', 'zur']);

/** "Jane Doe" gives "Doe"; "Doe, Jane" gives "Doe"; a single word gives nothing, it may be either. */
export function lastName(name: string): string {
	const trimmed = name.trim();
	if (trimmed.includes(',')) return trimmed.split(',')[0].trim();
	const words = trimmed.split(/\s+/);
	if (words.length < 2) return '';
	let start = words.length - 1;
	while (start > 1 && particles.has(words[start - 1].toLowerCase())) start -= 1;
	return words.slice(start).join(' ');
}

/**
 * The template's text with its placeholders filled in. Without a name for the recipient the name
 * placeholders stay as they are, so they are seen and can be filled in by hand.
 */
export function fillTemplate(html: string, name: string, today = new Date()): { html: string; unfilled: boolean } {
	const date = new Intl.DateTimeFormat(undefined, { dateStyle: 'long' }).format(today);
	let unfilled = false;
	const filled = html.replace(/\{\s*(first name|last name|name|date)\s*\}/gi, (whole, which: string) => {
		const key = which.toLowerCase();
		if (key === 'date') return escape(date);
		const value = key === 'name' ? name.trim() : key === 'last name' ? lastName(name) : firstName(name);
		if (!value) {
			unfilled = true;
			return whole;
		}
		return escape(value);
	});
	return { html: filled, unfilled };
}

/** The first words of a template, to tell it from the others: its text, paragraphs one after another. */
export function preview(html: string): string {
	const spaced = html.replace(/<\/(p|li|h\d|blockquote)>|<br\s*\/?>/gi, '$& ');
	return (new DOMParser().parseFromString(spaced, 'text/html').body.textContent ?? '').replace(/\s+/g, ' ').trim();
}
