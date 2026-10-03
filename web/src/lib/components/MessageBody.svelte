<script lang="ts">
	import { api, type Message } from '#lib/api.ts';
	import { notify } from '#lib/app.svelte.ts';
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
	let height = $state(120);

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
html{background:#fff;color:#1a1f2e}
body{margin:0;padding:4px 2px;font:16px/1.5 system-ui,sans-serif;overflow-wrap:anywhere}
img{max-width:100%;height:auto}
table{max-width:100%}
pre{white-space:pre-wrap;font:inherit;margin:0}
blockquote{margin:0 0 0 .8ex;border-left:2px solid #c9cfdd;padding-left:1ex;color:#55607a}
</style></head><body>${body}</body></html>`;
	});

	function escapeHtml(text: string) {
		return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
	}

	function measure() {
		const doc = frame?.contentDocument;
		if (doc?.documentElement) height = doc.documentElement.scrollHeight;
	}

	function onload() {
		measure();
		const doc = frame?.contentDocument;
		if (!doc?.body) return;
		// Images arriving late change the height.
		new ResizeObserver(measure).observe(doc.body);
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
<iframe
	bind:this={frame}
	title="Message from {message.from.name || message.from.address}"
	sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
	{srcdoc}
	{onload}
	style="height: {height}px"
></iframe>

<style>
	iframe {
		display: block;
		width: 100%;
		border: 0;
		border-radius: 8px;
		background: #fff;
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
