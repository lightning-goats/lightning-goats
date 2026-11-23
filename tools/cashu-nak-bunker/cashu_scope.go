package main

import (
	"context"
	"errors"
	"sync"

	"fiatjaf.com/nostr"
	"fiatjaf.com/nostr/nip44"
	"fiatjaf.com/nostr/nip46"
	"github.com/mailru/easyjson"
)

type cashuScope struct {
	client          nostr.PubKey
	owner           nostr.PubKey
	conversationKey [32]byte
	// The upstream session tracks recent request IDs. Keep scoped calls serial.
	mu sync.Mutex
}

func newCashuScope(key string, secret nostr.SecretKey) (*cashuScope, error) {
	client, err := nostr.PubKeyFromHex(key)
	owner := secret.Public()
	if err != nil || client == nostr.ZeroPK || client == owner || client.Hex() != key {
		return nil, errors.New("invalid dedicated Cashu client public key")
	}
	conversationKey, err := nip44.GenerateConversationKey(client, secret)
	if err != nil {
		return nil, errors.New("invalid dedicated Cashu client public key")
	}
	return &cashuScope{client: client, owner: owner, conversationKey: conversationKey}, nil
}

func (s *cashuScope) allows(request nip46.Request) bool {
	args := request.Params
	switch request.Method {
	case "connect":
		return (len(args) == 1 || len(args) == 2 && args[1] == "") && args[0] == s.owner.Hex()
	case "get_public_key", "ping":
		return len(args) == 0
	case "sign_event":
		if len(args) != 1 || len(args[0]) > 64_000 {
			return false
		}
		var event nostr.Event
		// Use the signing library's own event decoder for the authorization decision.
		return easyjson.Unmarshal([]byte(args[0]), &event) == nil &&
			(event.Kind == 10019 || event.Kind == 7376) &&
			(event.PubKey == nostr.ZeroPK || event.PubKey == s.owner)
	case "nip44_encrypt", "nip44_decrypt":
		return len(args) == 2 && args[0] == s.owner.Hex() && len(args[1]) > 0 && len(args[1]) <= 1024
	default:
		return false
	}
}

func (s *cashuScope) handle(ctx context.Context, signer nip46.Signer, secret nostr.SecretKey, event nostr.Event) (nostr.Event, error) {
	if event.PubKey != s.client || event.Kind != nostr.KindNostrConnect || len(event.Content) > 90_000 ||
		len(event.Tags) != 1 || len(event.Tags[0]) != 2 || event.Tags[0][0] != "p" || event.Tags[0][1] != s.owner.Hex() ||
		!event.CheckID() || !event.VerifySignature() {
		return nostr.Event{}, errors.New("invalid Cashu signing request event")
	}
	session := nip46.Session{PublicKey: s.owner, ConversationKey: s.conversationKey}
	request, err := session.ParseRequest(event)
	if err != nil || request.ID == "" || len(request.ID) > 128 {
		return nostr.Event{}, errors.New("invalid Cashu signing request")
	}
	if s.allows(request) {
		s.mu.Lock()
		defer s.mu.Unlock()
		_, _, response, err := signer.HandleRequest(ctx, event)
		return response, err
	}
	// Send the normal encrypted RPC refusal without invoking any project
	// signing or payload crypto requested by the caller.
	_, response, err := session.MakeResponse(request.ID, s.client, "", errors.New("unauthorized"))
	if err == nil {
		err = response.Sign(secret)
	}
	return response, err
}
