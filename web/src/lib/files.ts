import type { FileEntry } from './api.ts';

const base = (file: FileEntry) => `/api/messages/${file.message_id}/attachments/${file.idx}`;

export const downloadUrl = base;
export const thumbUrl = (file: FileEntry) => `${base(file)}/thumb`;
export const imageUrl = (file: FileEntry) => `${base(file)}/view`;

/** Where the PDF to draw comes from: the attachment itself, or its converted rendition. */
export function pdfUrl(file: FileEntry, officePreviews: boolean): string | null {
	if (file.kind === 'pdf') return base(file);
	if (file.kind === 'office' && officePreviews) return `${base(file)}/pdf`;
	return null;
}

export function extension(filename: string): string {
	const dot = filename.lastIndexOf('.');
	return dot > 0 ? filename.slice(dot + 1).toLowerCase().slice(0, 5) : 'file';
}
