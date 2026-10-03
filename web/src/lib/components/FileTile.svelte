<script lang="ts">
	import type { FileEntry } from '#lib/api.ts';
	import { extension, pdfUrl, thumbUrl } from '#lib/files.ts';
	import { displayName, shortDate } from '#lib/format.ts';

	let {
		file,
		officePreviews,
		onopen,
		caption
	}: { file: FileEntry; officePreviews: boolean; onopen: () => void; caption?: string } = $props();

	let failed = $state(false);
	let drawn = $state(false);
	const pdf = $derived(pdfUrl(file, officePreviews));

	/** Draws the first page once the tile is on screen. */
	function firstPage(canvas: HTMLCanvasElement, url: string) {
		const observer = new IntersectionObserver(
			async ([entry]) => {
				if (!entry.isIntersecting) return;
				observer.disconnect();
				try {
					const { loadPdf, renderPage, queued } = await import('#lib/pdf.ts');
					await queued(async () => {
						const doc = await loadPdf(url);
						try {
							await renderPage(doc, 1, canvas, canvas.clientWidth || 240);
						} finally {
							doc.loadingTask.destroy();
						}
					});
					drawn = true;
				} catch {
					failed = true;
				}
			},
			{ rootMargin: '200px' }
		);
		observer.observe(canvas);
		return { destroy: () => observer.disconnect() };
	}
</script>

<button class="tile" onclick={onopen}>
	<span class="preview" class:paper={drawn}>
		{#if file.kind === 'image' && !failed}
			<img src={thumbUrl(file)} alt="" loading="lazy" onerror={() => (failed = true)} />
		{:else if pdf && !failed}
			<canvas use:firstPage={pdf} class:drawn></canvas>
			{#if !drawn}<span class="type">{extension(file.filename)}</span>{/if}
		{:else}
			<span class="type">{extension(file.filename)}</span>
		{/if}
	</span>
	<span class="name">{file.filename}</span>
	<span class="from">{caption ?? `${displayName(file.from_name, file.from_addr)}, ${shortDate(file.date)}`}</span>
</button>

<style>
	.tile {
		display: grid;
		gap: 0.15rem;
		padding: 0;
		border: 0;
		background: none;
		text-align: left;
		cursor: pointer;
		min-width: 0;
	}
	.preview {
		display: grid;
		place-items: center;
		aspect-ratio: 4 / 3;
		margin-bottom: 0.35rem;
		border: 1px solid var(--line);
		border-radius: 10px;
		background: var(--surface);
		overflow: hidden;
		position: relative;
	}
	.preview.paper {
		background: #fff;
	}
	.tile:hover .preview {
		border-color: var(--important);
	}
	img,
	canvas {
		width: 100%;
		height: 100%;
		object-fit: cover;
		object-position: top;
	}
	canvas {
		position: absolute;
		inset: 0;
		opacity: 0;
	}
	canvas.drawn {
		opacity: 1;
	}
	.type {
		font: 700 1.1rem var(--display);
		color: var(--ink-soft);
		border: 2px solid var(--line);
		border-radius: 8px;
		padding: 0.35rem 0.7rem;
	}
	.name {
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.from {
		font-size: 0.85rem;
		color: var(--ink-soft);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
