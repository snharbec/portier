<script lang="ts">
	import { api } from '#lib/api.ts';
	import { pictureChanged } from '#lib/pictures.svelte.ts';
	import Avatar from './Avatar.svelte';

	let {
		senderId,
		address,
		name,
		hasPicture,
		onchange
	}: {
		senderId: number;
		address: string;
		name: string;
		/** A picture of the user's own is set at the moment. */
		hasPicture: boolean;
		/** Called after the picture was set (true) or removed (false). */
		onchange: (has: boolean) => void;
	} = $props();

	let url = $state('');
	let busy = $state(false);
	let error = $state('');

	async function change(request: () => Promise<unknown>, has: boolean) {
		busy = true;
		error = '';
		try {
			await request();
			pictureChanged(address);
			url = '';
			onchange(has);
		} catch (e) {
			error = (e as Error).message;
		} finally {
			busy = false;
		}
	}

	function upload(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		input.value = '';
		if (!file) return;
		if (file.size > 5 * 1024 * 1024) {
			error = 'The picture is too large. It may have up to 5 MB.';
			return;
		}
		const form = new FormData();
		form.append('file', file);
		change(() => api.post(`/senders/${senderId}/picture`, form), true);
	}

	function fromUrl(event: SubmitEvent) {
		event.preventDefault();
		change(() => api.post(`/senders/${senderId}/picture/url`, { url }), true);
	}
</script>

<div class="picture">
	<Avatar {name} seed={address} {address} size={56} />
	<div class="ways">
		<p class="muted">
			{hasPicture ? 'You chose this picture for' : 'Choose a picture of your own for'}
			{name}. It replaces the one looked up for the address.
		</p>
		<div class="row">
			<label class="btn small">
				Upload an image
				<input type="file" accept="image/png,image/jpeg,image/gif,image/webp" onchange={upload} disabled={busy} hidden />
			</label>
			{#if hasPicture}
				<button class="btn small quiet danger" disabled={busy} onclick={() => change(() => api.delete(`/senders/${senderId}/picture`), false)}>
					Remove your picture
				</button>
			{/if}
		</div>
		<form class="row" onsubmit={fromUrl}>
			<input
				class="input"
				type="url"
				bind:value={url}
				placeholder="https://… address of a picture on the web"
				aria-label="Web address of a picture"
				autocomplete="off"
				required
			/>
			<button class="btn small" disabled={busy}>{busy ? 'Loading' : 'Use this picture'}</button>
		</form>
		<p class="muted small">JPEG, PNG, GIF or WebP, up to 5 MB. Portier keeps a small copy; the web address is not asked again.</p>
		{#if error}<p class="error" role="alert">{error}</p>{/if}
	</div>
</div>

<style>
	.picture {
		display: flex;
		gap: 1rem;
		align-items: flex-start;
		padding: 0.8rem 1rem;
		border: 1px solid var(--line);
		border-radius: var(--radius);
		background: var(--surface);
		max-width: 44rem;
	}
	.ways {
		flex: 1;
		min-width: 0;
		display: grid;
		gap: 0.5rem;
	}
	.ways p {
		margin: 0;
		max-width: none;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		align-items: center;
	}
	.row .input {
		flex: 1 1 14rem;
		width: auto;
		padding-block: 0.3rem;
		font-size: 0.925rem;
	}
	.small {
		font-size: 0.85rem;
	}
</style>
