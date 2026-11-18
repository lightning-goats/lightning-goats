package main

import (
	"context"
	stdjson "encoding/json"
	"fmt"
	"testing"

	"fiatjaf.com/nostr"
	"fiatjaf.com/nostr/nip44"
	"fiatjaf.com/nostr/nip46"
)

type observedSigner struct {
	nip46.Signer
	calls int
}

func (s *observedSigner) HandleRequest(ctx context.Context, event nostr.Event) (nip46.Request, nip46.Response, nostr.Event, error) {
	s.calls++
	return s.Signer.HandleRequest(ctx, event)
}

func TestCashuSigningScope(t *testing.T) {
	ownerSecret := nostr.SecretKey{31: 1}
	clientSecret := nostr.SecretKey{31: 2}
	sibling := nostr.SecretKey{31: 3}
	owner, client := ownerSecret.Public(), clientSecret.Public()
	for _, key := range []string{"", owner.Hex(), nostr.ZeroPK.Hex()} {
		if _, err := newCashuScope(key, ownerSecret); err == nil {
			t.Fatal("invalid scoped key accepted")
		}
	}
	scope, err := newCashuScope(client.Hex(), ownerSecret)
	if err != nil {
		t.Fatal(err)
	}
	shared, err := nip44.GenerateConversationKey(owner, clientSecret)
	if err != nil {
		t.Fatal(err)
	}
	signer := nip46.NewStaticKeySigner(ownerSecret)
	signer.AuthorizeRequest = func(_ bool, from nostr.PubKey, _ string) bool { return from == client }
	downstream := &observedSigner{Signer: &signer}

	requestEvent := func(index int, method string, params []string) nostr.Event {
		plaintext, err := stdjson.Marshal(nip46.Request{ID: fmt.Sprintf("request-%d", index), Method: method, Params: params})
		if err != nil {
			t.Fatal(err)
		}
		payload, err := nip44.Encrypt(string(plaintext), shared)
		if err != nil {
			t.Fatal(err)
		}
		event := nostr.Event{Kind: nostr.KindNostrConnect, Content: payload, Tags: nostr.Tags{{"p", owner.Hex()}}}
		if err := event.Sign(clientSecret); err != nil {
			t.Fatal(err)
		}
		return event
	}
	eventJSON := func(kind int) string {
		return fmt.Sprintf(`{"created_at":100,"kind":%d,"tags":[],"content":"synthetic"}`, kind)
	}
	for index, test := range []struct {
		method string
		params []string
		allow  bool
	}{
		{"connect", []string{owner.Hex(), ""}, true},
		{"get_public_key", nil, true},
		{"ping", nil, true},
		{"sign_event", []string{eventJSON(10019)}, true},
		{"sign_event", []string{eventJSON(7376)}, true},
		{"nip44_encrypt", []string{owner.Hex(), "synthetic"}, true},
		{"sign_event", []string{eventJSON(1)}, false},
		{"sign_event", []string{eventJSON(9734)}, false},
		{"sign_event", []string{`{"kind":7376,"kind":1}`}, false},
		{"nip44_encrypt", []string{sibling.Public().Hex(), "synthetic"}, false},
		{"nip44_decrypt", []string{sibling.Public().Hex(), "synthetic"}, false},
		{"nip04_encrypt", []string{owner.Hex(), "synthetic"}, false},
		{"switch_relays", nil, false},
		{"connect", []string{owner.Hex(), "a-pairing-secret"}, false},
		{"imagined-nostrconnect", []string{client.Hex(), "pairing"}, false},
		{"unrecognized", nil, false},
	} {
		before := downstream.calls
		event := requestEvent(index, test.method, test.params)
		reply, err := scope.handle(t.Context(), downstream, ownerSecret, event)
		if err != nil {
			t.Fatal(err)
		}
		if reply.PubKey != owner || !reply.CheckID() || !reply.VerifySignature() {
			t.Fatal("invalid reply signature")
		}
		plaintext, err := nip44.Decrypt(reply.Content, shared)
		var response nip46.Response
		if err != nil || stdjson.Unmarshal([]byte(plaintext), &response) != nil || response.ID != fmt.Sprintf("request-%d", index) {
			t.Fatal("invalid reply envelope")
		}
		if test.allow {
			if response.Error != "" || response.Result == "" || downstream.calls != before+1 {
				t.Fatalf("approved method %s failed", test.method)
			}
		} else if response.Error != "unauthorized" || response.Result != "" || downstream.calls != before {
			t.Fatalf("denied method %s reached the signing backend", test.method)
		}
	}
	forged := requestEvent(100, "sign_event", []string{eventJSON(10019)})
	forged.Sig[0] ^= 1
	mistargeted := requestEvent(101, "sign_event", []string{eventJSON(10019)})
	mistargeted.Tags[0][1] = sibling.Public().Hex()
	if err := mistargeted.Sign(clientSecret); err != nil {
		t.Fatal(err)
	}
	before := downstream.calls
	for _, event := range []nostr.Event{forged, mistargeted} {
		if reply, err := scope.handle(t.Context(), downstream, ownerSecret, event); err == nil ||
			reply.PubKey != nostr.ZeroPK || downstream.calls != before {
			t.Fatal("invalid transport event reached the signer")
		}
	}
}
