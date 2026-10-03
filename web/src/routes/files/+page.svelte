<script lang="ts">
	import { api, type FileEntry, type FileList } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import FileTile from '#lib/components/FileTile.svelte';
	import FileViewer from '#lib/components/FileViewer.svelte';

	let list = $state<FileList | null>(null);
	let error = $state('');
	let open = $state<number | null>(null);

	$effect(() => {
		app.tick;
		api.get<FileList>('/attachments?weeks=4')
			.then((loaded) => (list = loaded))
			.catch((e) => (error = e.message));
	});

	const labels = ['This week', 'Last week', '2 weeks ago', '3 weeks ago'];

	/** Seven-day buckets counted back from now; `index` is the position in the flat list. */
	const groups = $derived.by(() => {
		const now = Date.now() / 1000;
		const buckets = labels.map((label) => ({ label, items: [] as { file: FileEntry; index: number }[] }));
		list?.files.forEach((file, index) => {
			const week = Math.min(3, Math.max(0, Math.floor((now - file.date) / (7 * 86400))));
			buckets[week].items.push({ file, index });
		});
		return buckets.filter((bucket) => bucket.items.length);
	});
</script>

<div class="page-head">
	<h1>Attachments</h1>
	<p>Files you received in the last four weeks from senders in Important and Nice to know.</p>
</div>

{#if error}
	<p class="error" role="alert">{error}</p>
{:else if list === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if list.files.length === 0}
	<div class="empty sheet">
		<strong>No attachments in the last four weeks</strong>
		Files sent by people you let in show up here, newest first.
	</div>
{:else}
	{#if !list.office_previews && list.files.some((f) => f.kind === 'office')}
		<p class="muted hint">
			Word, Excel and PowerPoint files show without a picture because LibreOffice is not installed on the server.
		</p>
	{/if}
	{#each groups as group (group.label)}
		<h2 class="section-title">{group.label}</h2>
		<div class="grid">
			{#each group.items as { file, index } (`${file.message_id}-${file.idx}`)}
				<FileTile {file} officePreviews={list.office_previews} onopen={() => (open = index)} />
			{/each}
		</div>
	{/each}
{/if}

{#if list && open !== null}
	<FileViewer files={list.files} bind:index={open} officePreviews={list.office_previews} onclose={() => (open = null)} />
{/if}

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(10.5rem, 1fr));
		gap: 1.25rem 1rem;
	}
	.hint {
		font-size: 0.9rem;
		margin: 0;
	}
</style>
