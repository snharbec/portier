import { getDocument, GlobalWorkerOptions, type PDFDocumentProxy } from 'pdfjs-dist';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

GlobalWorkerOptions.workerSrc = workerUrl;

/** Fetches a PDF as plain bytes (the server never serves it as a document) and parses it. */
export async function loadPdf(url: string): Promise<PDFDocumentProxy> {
	const response = await fetch(url, { credentials: 'same-origin' });
	if (!response.ok) {
		const data = await response.json().catch(() => null);
		throw new Error(data?.error ?? 'This document cannot be loaded');
	}
	const data = await response.arrayBuffer();
	return getDocument({ data }).promise;
}

/** Draws one page into `canvas`, `cssWidth` CSS pixels wide, sharp on high-density screens. */
export async function renderPage(doc: PDFDocumentProxy, pageNumber: number, canvas: HTMLCanvasElement, cssWidth: number) {
	const page = await doc.getPage(pageNumber);
	const base = page.getViewport({ scale: 1 });
	const density = Math.min(window.devicePixelRatio || 1, 2);
	const viewport = page.getViewport({ scale: (cssWidth / base.width) * density });
	canvas.width = Math.floor(viewport.width);
	canvas.height = Math.floor(viewport.height);
	await page.render({ canvas, canvasContext: canvas.getContext('2d')!, viewport }).promise;
}

// Tiles draw first pages as they scroll into view; keep that to two documents at a time.
const waiting: (() => void)[] = [];
let running = 0;

export async function queued<T>(job: () => Promise<T>): Promise<T> {
	if (running >= 2) await new Promise<void>((resolve) => waiting.push(resolve));
	running += 1;
	try {
		return await job();
	} finally {
		running -= 1;
		waiting.shift()?.();
	}
}
