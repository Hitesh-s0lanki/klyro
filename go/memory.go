package klyro

import (
	"context"
	"encoding/binary"
	"fmt"
	"math"
	"strconv"
	"strings"

	"github.com/redis/go-redis/v9"
)

type Mode string

const (
	Search Mode = "SEARCH"
	Vector Mode = "VECTOR"
	Hybrid Mode = "HYBRID"
)

type Metric string

const (
	Cosine       Metric = "COSINE"
	L2           Metric = "L2"
	InnerProduct Metric = "IP"
)

type Fusion string

const (
	Linear Fusion = "LINEAR"
	RRF    Fusion = "RRF"
)

type Condition string

const (
	NX Condition = "NX"
	XX Condition = "XX"
)

type Weights struct{ Keyword, Vector, Recency, Importance float64 }
type Filter struct {
	Field, Op string
	Value     any
}

type CreateOptions struct {
	Mode     Mode
	Dim      int
	Metric   Metric
	Weights  *Weights
	HalfLife int64
}
type ConfigOptions struct {
	Weights  *Weights
	HalfLife int64
}
type AddOptions struct {
	Text       string
	ID         string
	Vector     []float32
	Metadata   map[string]any
	Importance *float64
	TTL        int64
	Condition  Condition
}
type ReturnOptions struct{ NoText, WithMetadata, WithVector bool }
type SearchOptions struct {
	ReturnOptions
	TopK       int
	Filters    []Filter
	WithScores bool
}
type QueryOptions struct {
	SearchOptions
	Text    string
	Vector  []float32
	Weights *Weights
	Fusion  Fusion
}
type ScanOptions struct {
	Count   int
	Filters []Filter
}

type MemoryInfo struct {
	Mode                    Mode
	Dim                     int
	Metric                  Metric
	Weights                 Weights
	HalfLife                int64
	Records, Vectors, Terms int64
	AverageDocumentLength   float64
	Bytes                   int64
}
type MemoryRecord struct {
	ID                         string
	Text                       *string
	Importance                 float64
	CreatedAt, UpdatedAt, PTTL int64
	Metadata                   map[string]string
	Vector                     []float32
}
type MemoryHit struct {
	MemoryRecord
	Score                                   float64
	KeywordScore, VectorScore, RecencyScore *float64
}
type ScanResult struct {
	Cursor string
	IDs    []string
}

type MemoryClient struct{ redis redis.UniversalClient }

func (m *MemoryClient) call(ctx context.Context, command string, args ...any) (any, error) {
	all := append([]any{"MEM." + command}, args...)
	return m.redis.Do(ctx, all...).Result()
}

func (m *MemoryClient) Create(ctx context.Context, key string, o CreateOptions) error {
	mode := o.Mode
	if mode == "" {
		mode = Hybrid
	}
	metric := o.Metric
	if metric == "" {
		metric = Cosine
	}
	args := []any{key, "MODE", string(mode)}
	if o.Dim > 0 {
		args = append(args, "DIM", o.Dim)
	}
	args = append(args, "METRIC", string(metric))
	args = appendWeights(args, o.Weights)
	if o.HalfLife > 0 {
		args = append(args, "HALFLIFE", o.HalfLife)
	}
	_, err := m.call(ctx, "CREATE", args...)
	return err
}

func (m *MemoryClient) Info(ctx context.Context, key string) (MemoryInfo, error) {
	raw, err := m.call(ctx, "INFO", key)
	if err != nil {
		return MemoryInfo{}, err
	}
	v, err := mapping(raw)
	if err != nil {
		return MemoryInfo{}, err
	}
	w, err := mapping(v["weights"])
	if err != nil {
		return MemoryInfo{}, err
	}
	return MemoryInfo{Mode: Mode(text(v["mode"])), Dim: integer(v["dim"]), Metric: Metric(text(v["metric"])),
		Weights:  Weights{number(w["keyword"]), number(w["vector"]), number(w["recency"]), number(w["importance"])},
		HalfLife: int64(integer(v["halflife"])), Records: int64(integer(v["records"])), Vectors: int64(integer(v["vectors"])),
		Terms: int64(integer(v["terms"])), AverageDocumentLength: number(v["avg_doc_len"]), Bytes: int64(integer(v["bytes"]))}, nil
}

func (m *MemoryClient) Config(ctx context.Context, key string, o ConfigOptions) error {
	args := appendWeights([]any{key}, o.Weights)
	if o.HalfLife > 0 {
		args = append(args, "HALFLIFE", o.HalfLife)
	}
	if len(args) == 1 {
		return fmt.Errorf("klyro: weights or half-life is required")
	}
	_, err := m.call(ctx, "CONFIG", args...)
	return err
}
func (m *MemoryClient) Card(ctx context.Context, key string) (int64, error) {
	raw, err := m.call(ctx, "CARD", key)
	return int64(integer(raw)), err
}

func (m *MemoryClient) Add(ctx context.Context, key string, o AddOptions) (string, error) {
	args := []any{key, "TEXT", o.Text}
	if o.ID != "" {
		args = append(args, "ID", o.ID)
	}
	if o.Vector != nil {
		args = append(args, "VEC", encodeVector(o.Vector))
	}
	for k, v := range o.Metadata {
		args = append(args, "META", k, v)
	}
	if o.Importance != nil {
		args = append(args, "IMPORTANCE", *o.Importance)
	}
	if o.TTL > 0 {
		args = append(args, "TTL", o.TTL)
	}
	if o.Condition != "" {
		args = append(args, string(o.Condition))
	}
	raw, err := m.call(ctx, "ADD", args...)
	return text(raw), err
}

func (m *MemoryClient) Get(ctx context.Context, key, id string, o ReturnOptions) (*MemoryRecord, error) {
	args := appendReturns([]any{key, id}, o, false)
	raw, err := m.call(ctx, "GET", args...)
	if err != nil || raw == nil {
		return nil, err
	}
	record, _, err := decodeRecord(raw)
	return &record, err
}
func (m *MemoryClient) MGet(ctx context.Context, key string, ids ...string) ([]*MemoryRecord, error) {
	if len(ids) == 0 {
		return nil, fmt.Errorf("klyro: at least one id is required")
	}
	args := []any{key}
	for _, id := range ids {
		args = append(args, id)
	}
	raw, err := m.call(ctx, "MGET", args...)
	if err != nil {
		return nil, err
	}
	items, err := sequence(raw)
	if err != nil {
		return nil, err
	}
	out := make([]*MemoryRecord, len(items))
	for i, item := range items {
		if item != nil {
			rec, _, e := decodeRecord(item)
			if e != nil {
				return nil, e
			}
			out[i] = &rec
		}
	}
	return out, nil
}
func (m *MemoryClient) Delete(ctx context.Context, key string, ids ...string) (int64, error) {
	return m.countCall(ctx, "DEL", key, ids)
}
func (m *MemoryClient) SetMetadata(ctx context.Context, key, id string, values map[string]any) (int64, error) {
	if len(values) == 0 {
		return 0, fmt.Errorf("klyro: metadata must not be empty")
	}
	args := []any{key, id}
	for k, v := range values {
		args = append(args, k, v)
	}
	raw, err := m.call(ctx, "SETMETA", args...)
	return int64(integer(raw)), err
}
func (m *MemoryClient) DeleteMetadata(ctx context.Context, key, id string, fields ...string) (int64, error) {
	if len(fields) == 0 {
		return 0, fmt.Errorf("klyro: at least one field is required")
	}
	args := []any{key, id}
	for _, f := range fields {
		args = append(args, f)
	}
	raw, err := m.call(ctx, "DELMETA", args...)
	return int64(integer(raw)), err
}
func (m *MemoryClient) Expire(ctx context.Context, key, id string, seconds int64) (bool, error) {
	raw, err := m.call(ctx, "EXPIRE", key, id, seconds)
	return integer(raw) == 1, err
}

func (m *MemoryClient) Scan(ctx context.Context, key, cursor string, o ScanOptions) (ScanResult, error) {
	args := []any{key, cursor}
	if o.Count > 0 {
		args = append(args, "COUNT", o.Count)
	}
	args = appendFilters(args, o.Filters)
	raw, err := m.call(ctx, "SCAN", args...)
	if err != nil {
		return ScanResult{}, err
	}
	pair, err := sequence(raw)
	if err != nil || len(pair) != 2 {
		return ScanResult{}, fmt.Errorf("klyro: invalid scan reply")
	}
	ids, err := sequence(pair[1])
	if err != nil {
		return ScanResult{}, err
	}
	out := ScanResult{Cursor: text(pair[0]), IDs: make([]string, len(ids))}
	for i := range ids {
		out.IDs[i] = text(ids[i])
	}
	return out, nil
}
func (m *MemoryClient) Search(ctx context.Context, key, query string, o SearchOptions) ([]MemoryHit, error) {
	return m.hits(ctx, "SEARCH", appendRetrieval([]any{key, query}, o)...)
}
func (m *MemoryClient) VectorSearch(ctx context.Context, key string, vector []float32, o SearchOptions) ([]MemoryHit, error) {
	return m.hits(ctx, "VSEARCH", appendRetrieval([]any{key, "VEC", encodeVector(vector)}, o)...)
}
func (m *MemoryClient) Query(ctx context.Context, key string, o QueryOptions) ([]MemoryHit, error) {
	if o.Text == "" && o.Vector == nil {
		return nil, fmt.Errorf("klyro: text or vector is required")
	}
	args := []any{key}
	if o.Text != "" {
		args = append(args, "TEXT", o.Text)
	}
	if o.Vector != nil {
		args = append(args, "VEC", encodeVector(o.Vector))
	}
	if o.Fusion != "" {
		args = append(args, "FUSION", string(o.Fusion))
	}
	args = appendWeights(args, o.Weights)
	args = appendRetrieval(args, o.SearchOptions)
	return m.hits(ctx, "QUERY", args...)
}

func (m *MemoryClient) hits(ctx context.Context, command string, args ...any) ([]MemoryHit, error) {
	raw, err := m.call(ctx, command, args...)
	if err != nil {
		return nil, err
	}
	items, err := sequence(raw)
	if err != nil {
		return nil, err
	}
	out := make([]MemoryHit, len(items))
	for i, item := range items {
		_, hit, e := decodeRecord(item)
		if e != nil {
			return nil, e
		}
		out[i] = hit
	}
	return out, nil
}
func (m *MemoryClient) countCall(ctx context.Context, command, key string, values []string) (int64, error) {
	if len(values) == 0 {
		return 0, fmt.Errorf("klyro: at least one item is required")
	}
	args := []any{key}
	for _, v := range values {
		args = append(args, v)
	}
	raw, err := m.call(ctx, command, args...)
	return int64(integer(raw)), err
}

func appendWeights(args []any, w *Weights) []any {
	if w != nil {
		args = append(args, "WEIGHTS", w.Keyword, w.Vector, w.Recency, w.Importance)
	}
	return args
}
func appendFilters(args []any, filters []Filter) []any {
	for _, f := range filters {
		args = append(args, "FILTER", f.Field, strings.ToUpper(f.Op), f.Value)
	}
	return args
}
func appendReturns(args []any, o ReturnOptions, scores bool) []any {
	if o.NoText {
		args = append(args, "NOTEXT")
	}
	if o.WithMetadata {
		args = append(args, "WITHMETA")
	}
	if o.WithVector {
		args = append(args, "WITHVEC")
	}
	if scores {
		args = append(args, "WITHSCORES")
	}
	return args
}
func appendRetrieval(args []any, o SearchOptions) []any {
	if o.TopK > 0 {
		args = append(args, "TOPK", o.TopK)
	}
	args = appendFilters(args, o.Filters)
	return appendReturns(args, o.ReturnOptions, o.WithScores)
}
func encodeVector(values []float32) []byte {
	out := make([]byte, len(values)*4)
	for i, v := range values {
		binary.LittleEndian.PutUint32(out[i*4:], math.Float32bits(v))
	}
	return out
}
func decodeVector(raw any) ([]float32, error) {
	var b []byte
	switch value := raw.(type) {
	case []byte:
		b = value
	case string:
		b = []byte(value)
	default:
		return nil, fmt.Errorf("klyro: invalid vector reply")
	}
	if len(b)%4 != 0 {
		return nil, fmt.Errorf("klyro: invalid vector reply")
	}
	out := make([]float32, len(b)/4)
	for i := range out {
		out[i] = math.Float32frombits(binary.LittleEndian.Uint32(b[i*4:]))
	}
	return out, nil
}
func text(v any) string {
	switch x := v.(type) {
	case string:
		return x
	case []byte:
		return string(x)
	default:
		return fmt.Sprint(x)
	}
}
func number(v any) float64 {
	switch x := v.(type) {
	case float64:
		return x
	case int64:
		return float64(x)
	default:
		n, _ := strconv.ParseFloat(text(x), 64)
		return n
	}
}
func integer(v any) int {
	switch x := v.(type) {
	case int:
		return x
	case int64:
		return int(x)
	default:
		n, _ := strconv.Atoi(text(x))
		return n
	}
}
func sequence(v any) ([]any, error) {
	if x, ok := v.([]any); ok {
		return x, nil
	}
	return nil, fmt.Errorf("klyro: invalid array reply")
}
func mapping(v any) (map[string]any, error) {
	if x, ok := v.(map[any]any); ok {
		out := map[string]any{}
		for k, val := range x {
			out[text(k)] = val
		}
		return out, nil
	}
	if x, ok := v.(map[string]any); ok {
		return x, nil
	}
	seq, err := sequence(v)
	if err != nil || len(seq)%2 != 0 {
		return nil, fmt.Errorf("klyro: invalid map reply")
	}
	out := map[string]any{}
	for i := 0; i < len(seq); i += 2 {
		out[text(seq[i])] = seq[i+1]
	}
	return out, nil
}
func decodeRecord(raw any) (MemoryRecord, MemoryHit, error) {
	v, err := mapping(raw)
	if err != nil {
		return MemoryRecord{}, MemoryHit{}, err
	}
	r := MemoryRecord{ID: text(v["id"]), Importance: number(v["importance"]), CreatedAt: int64(integer(v["created_at"])), UpdatedAt: int64(integer(v["updated_at"])), PTTL: int64(integer(v["pttl"]))}
	if x, ok := v["text"]; ok {
		s := text(x)
		r.Text = &s
	}
	if x, ok := v["meta"]; ok {
		values, e := mapping(x)
		if e != nil {
			return r, MemoryHit{}, e
		}
		r.Metadata = map[string]string{}
		for k, value := range values {
			r.Metadata[k] = text(value)
		}
	}
	if x, ok := v["vector"]; ok && x != nil {
		r.Vector, err = decodeVector(x)
		if err != nil {
			return r, MemoryHit{}, err
		}
	}
	h := MemoryHit{MemoryRecord: r, Score: number(v["score"])}
	for name, target := range map[string]**float64{"keyword_score": &h.KeywordScore, "vector_score": &h.VectorScore, "recency_score": &h.RecencyScore} {
		if x, ok := v[name]; ok {
			n := number(x)
			*target = &n
		}
	}
	return r, h, nil
}
