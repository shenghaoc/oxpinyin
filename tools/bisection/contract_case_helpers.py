"""Token phrases and SingleGram encoding for the cross-open cases (#1175)."""

TOKEN_REUSE_PHRASES = (
    ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'), ('龖龖', "ta4'ta4", 'ㄊㄚˋ ㄊㄚˋ'),
    ('靐靐', "bing3'bing3", 'ㄅㄧㄥˇ ㄅㄧㄥˇ'), ('鱻鱻', "xian1'xian1", 'ㄒㄧㄢ ㄒㄧㄢ'),
    ('麤麤', "cu1'cu1", 'ㄘㄨ ㄘㄨ'), ('厵厵', "yuan2'yuan2", 'ㄩㄢˊ ㄩㄢˊ'),
)


def d18_gram(total, token, count):
    """Encode one SingleGram total/token/count triple as unsigned 32-bit little-endian words.

    Follow SingleGram::get_total_freq and ::retrieve_all in src/storage/ngram.cpp:48-50,133-151
    at pin 074a2219; out-of-range integers fail through to_bytes."""
    return b''.join(n.to_bytes(4, 'little') for n in (total, token, count))
