import { notify } from './app.svelte.ts';

/** Copies text and says so. Falls back for pages not served over HTTPS, where the clipboard API is absent. */
export async function copyText(text: string) {
	try {
		if (navigator.clipboard) {
			await navigator.clipboard.writeText(text);
		} else {
			const field = document.createElement('textarea');
			field.value = text;
			field.setAttribute('readonly', '');
			field.style.position = 'fixed';
			field.style.opacity = '0';
			document.body.append(field);
			field.select();
			const copied = document.execCommand('copy');
			field.remove();
			if (!copied) throw new Error('copy refused');
		}
		notify(`Copied ${text}`);
	} catch {
		notify(`Could not copy. The address is ${text}`);
	}
}
