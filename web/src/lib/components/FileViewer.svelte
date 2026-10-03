<script lang="ts">
	import type { FileEntry } from '#lib/api.ts';
	import { downloadUrl, imageUrl, pdfUrl } from '#lib/files.ts';
	import { displayName, fileSize, fullDate } from '#lib/format.ts';
	import type { PDFDocumentProxy } from 'pdfjs-dist';
	import { onMount } from 'svelte';

	let {
		files,
		index = $bindable(),
		officePreviews,
		onclose,
		emailLink = true
	}: {
		files: FileEntry[];
		index: number;
		officePreviews: boolean;
		onclose: () => void;
		/** Off when the viewer is opened from inside the email itself. */
		emailLink?: boolean;
	} = $props();

	let dialog: HTMLDialogElement;
	let pages: HTMLDivElement | undefined = $state();
	let doc = $state<PDFDocumentProxy | null>(null);
	let ratio = $state(1.414);
	let error = $state('');
	let loading = $state(false);

	const file = $derived(files[index]);
	const pdf = $derived(pdfUrl(file, officePreviews));

	onMount(() => dialog.showModal());

	// Load the document whenever another file is shown. The effect must depend on the URL only:
	// it writes `doc`, so reading `doc` here would make it restart itself.
	$effect(() => {
		const url = pdf;
		error = '';
		if (!url) {
			loading = false;
			return;
		}
		let stale = false;
		let loaded: PDFDocumentProxy | null = null;
		loading = true;
		import('#lib/pdf.ts')
			.then(({ loadPdf }) => loadPdf(url))
			.then(async (opened) => {
				if (stale) return void opened.loadingTask.destroy();
				loaded = opened;
				const first = (await opened.getPage(1)).getViewport({ scale: 1 });
				ratio = first.height / first.width;
				doc = opened;
			})
			.catch((e) => {
				if (!stale) error = e.message;
			})
			.finally(() => {
				if (!stale) loading = false;
			});
		return () => {
			stale = true;
			doc = null;
			loaded?.loadingTask.destroy();
		};
	});

	/** Draws a page when it scrolls into view, so long documents open at once. */
	function page(canvas: HTMLCanvasElement, params: { doc: PDFDocumentProxy; number: number }) {
		const observer = new IntersectionObserver(
			async ([entry]) => {
				if (!entry.isIntersecting) return;
				observer.disconnect();
				const { renderPage } = await import('#lib/pdf.ts');
				await renderPage(params.doc, params.number, canvas, canvas.clientWidth).catch(() => {});
			},
			{ root: pages, rootMargin: '600px' }
		);
		observer.observe(canvas);
		return { destroy: () => observer.disconnect() };
	}

	function step(by: number) {
		const next = index + by;
		if (next >= 0 && next < files.length) index = next;
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'ArrowLeft') step(-1);
		else if (event.key === 'ArrowRight') step(1);
	}

	const whyNoPreview = $derived(
		file.kind === 'office' && !officePreviews
			? 'Office documents can be shown here once LibreOffice is installed on the server.'
			: 'This kind of file cannot be shown here.'
	);
</script>

<dialog bind:this={dialog} {onclose} {onkeydown} aria-label={file.filename}>
	<header>
		<div class="what">
			<strong>{file.filename}</strong>
			<span class="muted">
				{displayName(file.from_name, file.from_addr)}, {fullDate(file.date)}, {fileSize(file.size)}
			</span>
		</div>
		{#if emailLink}<a class="btn small" href="/thread/{file.thread_id}">Open email</a>{/if}
		<a class="btn small" href={downloadUrl(file)} download={file.filename}>Download</a>
		<button class="btn small primary" onclick={() => dialog.close()}>Close</button>
	</header>

	<div class="stage" bind:this={pages}>
		{#if file.kind === 'image'}
			<img src={imageUrl(file)} alt={file.filename} />
		{:else if doc}
			{#key doc}
				{#each { length: doc.numPages } as _, i}
					<canvas use:page={{ doc, number: i + 1 }} style="aspect-ratio: 1 / {ratio}"></canvas>
				{/each}
			{/key}
		{:else if loading}
			<p class="note" aria-busy="true">
				{file.kind === 'office' ? 'Preparing the document. The first time takes a few seconds.' : 'Loading'}
			</p>
		{:else}
			<div class="note">
				<strong>No preview for {file.filename}</strong>
				<p>{error || whyNoPreview}</p>
				<a class="btn primary" href={downloadUrl(file)} download={file.filename}>Download the file</a>
			</div>
		{/if}
	</div>

	<footer>
		<button class="btn small" onclick={() => step(-1)} disabled={index === 0}>Previous</button>
		<span class="muted">{index + 1} of {files.length}</span>
		<button class="btn small" onclick={() => step(1)} disabled={index === files.length - 1}>Next</button>
	</footer>
</dialog>

<style>
	dialog {
		width: min(60rem, 100vw - 1.5rem);
		height: min(100vh - 1.5rem, 70rem);
		max-width: none;
		max-height: none;
		padding: 0;
		border: 1px solid var(--line);
		border-radius: var(--radius);
		background: var(--paper);
		color: var(--ink);
		overflow: hidden;
	}
	dialog[open] {
		display: grid;
		grid-template-rows: auto 1fr auto;
	}
	dialog::backdrop {
		background: color-mix(in srgb, var(--ink) 55%, transparent);
	}
	header,
	footer {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.7rem 1rem;
		background: var(--surface);
	}
	header {
		border-bottom: 1px solid var(--line);
		flex-wrap: wrap;
	}
	footer {
		border-top: 1px solid var(--line);
		justify-content: center;
		gap: 1rem;
	}
	.what {
		display: grid;
		flex: 1;
		min-width: 10rem;
	}
	.what strong,
	.what span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.what span {
		font-size: 0.85rem;
	}
	.stage {
		overflow: auto;
		padding: 1rem;
		display: grid;
		gap: 1rem;
		align-content: start;
		justify-items: center;
	}
	img {
		max-width: 100%;
		height: auto;
		border-radius: 6px;
		background: #fff;
	}
	canvas {
		width: min(100%, 52rem);
		background: #fff;
		border: 1px solid var(--line);
	}
	.note {
		align-self: center;
		text-align: center;
		padding: 3rem 1rem;
		color: var(--ink-soft);
	}
	.note strong {
		font: 700 1.25rem var(--display);
		color: var(--ink);
	}
</style>
