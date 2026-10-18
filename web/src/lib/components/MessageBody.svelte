<script lang="ts">
	import { api, type Message } from '#lib/api.ts';
	import { notify } from '#lib/app.svelte.ts';
	import { look } from '#lib/look.svelte.ts';
	import { untrack } from 'svelte';

	let { message }: { message: Message } = $props();

	/** The sender is one whose images are loaded without asking. */
	let remembered = $state(untrack(() => message.show_images));
	let showImages = $state(untrack(() => message.show_images));

	/** Showing images is remembered for the sender; hiding them again forgets it. */
	async function setImages(show: boolean) {
		showImages = show;
		if (message.sender_id === null) return;
		try {
			await api.post(`/senders/${message.sender_id}/images`, { show });
			remembered = show;
		} catch (e) {
			notify(`The choice could not be saved for this sender: ${(e as Error).message}`);
		}
	}
	let frame: HTMLIFrameElement | undefined = $state();
	let height = $state(24);
	/** How far a mail wider than the screen is shrunk to fit it; 1 when it fits as it is. */
	let scale = $state(1);
	/** The mail is laid out wider than its frame and was changed to fit (rewrapped or shrunk). */
	let fitted = $state(false);
	/** Some of its tables are too wide even so, and scroll sideways on their own. */
	let scrolling = $state(false);
	/** The reader asked for the mail as it was laid out, wider than the screen or not. */
	let actualSize = $state(false);

	// A mail that brings no layout or colours of its own (plain text, or just paragraphs and links)
	// is shown in the app's colours and at a readable line length. Designed mails keep their white page.
	const plain = $derived(!/<(table|img|style|font|center)\b|\sstyle=|\sbgcolor=|\sclass=/i.test(message.body_html));
	const fallback = { ink: '#1a1f2e', soft: '#55607a', line: '#c9cfdd', link: '#2f4be0' };
	let theme = $state(fallback);

	// The app's colours as they are now. Read once per mail and again when the look changes; this
	// must not read `theme` itself, or writing it would run the effect again without end.
	$effect(() => {
		message.id;
		look.theme;
		look.dark;
		const css = getComputedStyle(document.documentElement);
		const token = (name: string, otherwise: string) => css.getPropertyValue(name).trim() || otherwise;
		theme = {
			ink: token('--ink', fallback.ink),
			soft: token('--ink-soft', fallback.soft),
			line: token('--line', fallback.line),
			link: token('--important', fallback.link)
		};
	});

	const hasRemoteImages = $derived(/<img[^>]+src="https?:/i.test(message.body_html));

	// Second layer after server-side sanitising: no scripts, no network except what is listed.
	const srcdoc = $derived.by(() => {
		const images = showImages ? "img-src 'self' data: https: http:" : "img-src 'self' data:";
		const csp = `default-src 'none'; style-src 'unsafe-inline'; ${images}`;
		const body = message.body_html || `<pre>${escapeHtml(message.body_text)}</pre>`;
		return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${csp}">
<base target="_blank">
<style>
html{background:${plain ? 'transparent' : '#fff'};color:${plain ? theme.ink : '#1a1f2e'};color-scheme:${plain ? (look.dark ? 'dark' : 'light') : 'light'}}
body{margin:0;padding:4px 2px;font:16px/1.55 system-ui,sans-serif;overflow-wrap:anywhere;${plain ? 'max-width:46rem' : ''}}
${plain ? `a{color:${theme.link}}` : ''}
img{max-width:100%;height:auto}
table{max-width:100%}
pre{white-space:pre-wrap;font:inherit;margin:0}
${actualSize ? '' : `</style><style id="fit" media="not all">${narrow}`}
blockquote{margin:0 0 0 .8ex;border-left:2px solid ${plain ? theme.line : '#c9cfdd'};padding-left:1ex;color:${plain ? theme.soft : '#55607a'}}
</style></head><body>${body}</body></html>`;
	});

	// For a mail laid out wider than its frame (a phone, a narrow pane): it gives up its fixed widths,
	// so its text wraps within the frame. Columns side by side stay side by side as long as they fit;
	// what still does not fit is shrunk afterwards. Only switched on when needed (see fit).
	const narrow = `
table,tbody,thead,tfoot,tr,td,th,div,p,h1,h2,h3,center{max-width:100%!important;box-sizing:border-box}
table{width:auto!important;min-width:0!important;table-layout:auto!important}
div,p,center,h1,h2,h3{width:auto!important;min-width:0!important}
td,th{width:auto!important;min-width:0!important;white-space:normal!important;overflow-wrap:break-word!important}
td a,th a{overflow-wrap:anywhere!important}
img,video{max-width:100%!important;height:auto!important}
pre,code{white-space:pre-wrap!important}
table.portier-scroll{display:block!important;overflow-x:auto!important}`;

	/**
	 * Pictures may shrink with the column they are in: a fixed width would keep the table around them
	 * as wide as the picture, so it becomes "the column's width, at most the picture's own". Small
	 * ones (icons, logos in a line of text) stay as they are.
	 */
	function shrinkablePictures(doc: Document) {
		for (const image of doc.querySelectorAll('img')) {
			const width = image.getBoundingClientRect().width;
			if (width < 100) continue;
			image.dataset.portierStyle = image.getAttribute('style') ?? '';
			image.style.setProperty('width', '100%', 'important');
			image.style.setProperty('max-width', `${Math.round(width)}px`, 'important');
			image.style.setProperty('height', 'auto', 'important');
		}
	}

	/** The pictures as the mail has them, for the frame being wide enough after all. */
	function restorePictures(doc: Document) {
		for (const image of doc.querySelectorAll<HTMLImageElement>('img[data-portier-style]')) {
			image.setAttribute('style', image.dataset.portierStyle ?? '');
			delete image.dataset.portierStyle;
		}
	}

	/** Long runs without spaces (links written out) may break after / ? & = and the like. */
	function breakLongWords(doc: Document) {
		const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
		const long: Text[] = [];
		for (let node = walker.nextNode(); node; node = walker.nextNode()) {
			if (/\S{30,}/.test(node.nodeValue ?? '') && node.parentElement?.localName !== 'style') long.push(node as Text);
		}
		for (const node of long) {
			const parts = (node.nodeValue ?? '').split(/(?<=[/?&=._-])(?=[^/?&=._-])/);
			if (parts.length < 2) continue;
			const pieces = doc.createDocumentFragment();
			parts.forEach((part, index) => {
				if (index) pieces.append(doc.createElement('wbr'));
				pieces.append(part);
			});
			node.replaceWith(pieces);
		}
	}

	/**
	 * Tables of figures still wider than the frame scroll sideways on their own, the rest of the mail
	 * stays readable. Only the innermost wide table with three or more columns: a table that only
	 * lays out the mail is never one, else the whole mail would scroll sideways again.
	 */
	function scrollWideTables(doc: Document, room: number) {
		const wide = [...doc.querySelectorAll('table')].filter((table) => table.getBoundingClientRect().width > room + 1);
		const inner = wide.filter((table) => !wide.some((other) => other !== table && table.contains(other)));
		for (const table of inner as HTMLTableElement[]) {
			if ([...table.rows].some((row) => row.cells.length >= 3)) table.classList.add('portier-scroll');
		}
	}

	function escapeHtml(text: string) {
		return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
	}

	function measure() {
		const doc = frame?.contentDocument;
		if (doc?.documentElement) height = doc.documentElement.scrollHeight;
	}

	/** A mail still wider than its frame is shrunk until it fits, so nothing needs scrolling sideways. */
	function fit() {
		const doc = frame?.contentDocument;
		if (!doc?.body) return;
		doc.body.style.zoom = '';
		const room = doc.documentElement.clientWidth;
		// As it was laid out, is the mail wider than its frame? Then it is rewrapped.
		const rules = doc.getElementById('fit') as HTMLStyleElement | null;
		if (rules) {
			rules.media = 'not all';
			for (const table of doc.querySelectorAll('.portier-scroll')) table.classList.remove('portier-scroll');
			restorePictures(doc);
			fitted = doc.documentElement.scrollWidth > room + 1;
			rules.media = fitted ? 'all' : 'not all';
			if (fitted) {
				if (!doc.body.dataset.broken) breakLongWords(doc);
				doc.body.dataset.broken = '1';
				shrinkablePictures(doc);
				if (doc.documentElement.scrollWidth > room + 1) scrollWideTables(doc, room);
			}
			scrolling = doc.querySelector('table.portier-scroll') !== null;
		}
		// What still does not fit is shrunk, a little more if the first try leaves a few pixels over.
		scale = 1;
		for (let round = 0; !actualSize && round < 5; round++) {
			const wide = doc.documentElement.scrollWidth;
			if (wide <= room + 1) break;
			scale = Math.max(0.25, (scale * (room - 2)) / wide);
			doc.body.style.zoom = String(scale);
		}
		if (scale < 1) fitted = true;
		// A few pixels left over (rounding, a border) are cut off rather than scrolled to.
		const over = doc.documentElement.scrollWidth - room;
		doc.documentElement.style.overflowX = !actualSize && over > 0 && over <= 8 ? 'hidden' : '';
		measure();
	}

	function onload() {
		fit();
		const doc = frame?.contentDocument;
		if (!doc?.body || !frame) return;
		// Images arriving late change the height, and may make the mail wider.
		let pending = 0;
		new ResizeObserver(() => {
			measure();
			cancelAnimationFrame(pending);
			pending = requestAnimationFrame(() => scale === 1 && fit());
		}).observe(doc.body);
		// A picture arriving late may make the mail wider than its frame.
		doc.addEventListener(
			'load',
			() => {
				cancelAnimationFrame(pending);
				pending = requestAnimationFrame(fit);
			},
			true
		);
		// A turned phone or a dragged divider changes the room there is.
		let width = frame.clientWidth;
		new ResizeObserver(() => {
			if (!frame || frame.clientWidth === width) return;
			width = frame.clientWidth;
			fit();
		}).observe(frame);
		// Keys pressed after clicking into the mail still reach the page's shortcuts.
		doc.addEventListener('keydown', (event) => {
			const { key, code, shiftKey, ctrlKey, altKey, metaKey } = event;
			const copy = new KeyboardEvent('keydown', { key, code, shiftKey, ctrlKey, altKey, metaKey, bubbles: true, cancelable: true });
			if (!frame?.dispatchEvent(copy)) event.preventDefault();
		});
	}
</script>

{#if hasRemoteImages && !showImages}
	<p class="images">
		Images from the internet are hidden so senders cannot see that you opened this.
		<button class="btn small" onclick={() => setImages(true)}>Show images</button>
		{#if message.sender_id !== null}<span>They will then always be shown for this sender.</span>{/if}
	</p>
{:else if hasRemoteImages && remembered}
	<p class="images">
		Images from this sender are shown automatically.
		<button class="btn small" onclick={() => setImages(false)}>Hide images and ask again</button>
	</p>
{/if}
{#if actualSize}
	<p class="images">
		Shown as it was laid out: scroll sideways to read it all.
		<button class="btn small" onclick={() => (actualSize = false)}>Fit to screen</button>
	</p>
{:else if fitted}
	<p class="images">
		{scale < 1 ? 'Shrunk' : 'Rearranged'} to fit the screen{scrolling ? '; wide tables scroll sideways' : ''}.
		<button class="btn small" onclick={() => (actualSize = true)}>Original layout</button>
	</p>
{/if}
<!-- A changed mail gets a new frame: loading other content into the old one would add an entry
     ("about:srcdoc") to the browser's history, and Back would then step through mail frames. -->
{#key srcdoc}
<iframe
	bind:this={frame}
	title="Message from {message.from.name || message.from.address}"
	sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
	class:plain
	{srcdoc}
	{onload}
	style="height: {height}px"
></iframe>
{/key}

<style>
	iframe {
		display: block;
		width: 100%;
		border: 0;
		border-radius: 8px;
		background: #fff;
	}
	iframe.plain {
		background: transparent;
	}
	/* A phone: a designed mail uses the card's full width, every pixel counts there. */
	@media (max-width: 44rem) {
		iframe:not(.plain) {
			width: calc(100% + 1.5rem);
			margin-inline: -0.75rem;
			border-radius: 0;
		}
	}
	.images {
		margin: 0 0 0.75rem;
		font-size: 0.875rem;
		color: var(--ink-soft);
		display: flex;
		gap: 0.75rem;
		align-items: center;
		flex-wrap: wrap;
	}
</style>
