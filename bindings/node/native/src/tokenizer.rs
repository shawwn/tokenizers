extern crate tokenizers as tk;

use crate::models::JsModel;
use crate::tasks::tokenizer::{EncodeTask, WorkingTokenizer};
use crate::trainers::JsTrainer;
use neon::prelude::*;

/// Tokenizer
pub struct Tokenizer {
    tokenizer: tk::tokenizer::Tokenizer,

    /// Whether we have a running task. We keep this to make sure we never
    /// modify the underlying tokenizer while a task is running
    running_task: std::sync::Arc<()>,
}

impl Tokenizer {
    pub fn prepare_for_task(&self) -> WorkingTokenizer {
        unsafe { WorkingTokenizer::new(&self.tokenizer, self.running_task.clone()) }
    }
}

declare_types! {
    pub class JsTokenizer for Tokenizer {
        init(mut cx) {
            // init(model: JsModel)
            let mut model = cx.argument::<JsModel>(0)?;
            if let Some(instance) = {
                let guard = cx.lock();
                let mut model = model.borrow_mut(&guard);
                model.model.to_pointer()
            } {
                Ok(Tokenizer {
                    tokenizer: tk::tokenizer::Tokenizer::new(instance),
                    running_task: std::sync::Arc::new(())
                })
            } else {
                cx.throw_error("The Model is already being used in another Tokenizer")
            }
        }

        method runningTasks(mut cx) {
            // runningTasks(): number
            let running = {
                let this = cx.this();
                let guard = cx.lock();
                let count = std::sync::Arc::strong_count(&this.borrow(&guard).running_task);
                if count > 0 { count - 1 } else { 0 }
            };
            Ok(cx.number(running as f64).upcast())
        }

        method getVocabSize(mut cx) {
            // getVocabSize(withAddedTokens: bool = true)
            let mut with_added_tokens = true;
            if let Some(args) = cx.argument_opt(0) {
                with_added_tokens = args.downcast::<JsBoolean>().or_throw(&mut cx)?.value() as bool;
            }

            let mut this = cx.this();
            let guard = cx.lock();
            let size = this.borrow_mut(&guard).tokenizer.get_vocab_size(with_added_tokens);

            Ok(cx.number(size as f64).upcast())
        }

        method encode(mut cx) {
            // encode(sentence: String, pair: String | null = null, __callback: (err, encoding) -> void)
            let sentence = cx.argument::<JsString>(0)?.value();
            let mut pair: Option<String> = None;
            if let Some(args) = cx.argument_opt(1) {
                if let Ok(p) = args.downcast::<JsString>() {
                    pair = Some(p.value());
                } else if let Err(_) = args.downcast::<JsNull>() {
                    return cx.throw_error("Second arg must be of type `String | null`");
                }
            }
            let callback = cx.argument::<JsFunction>(2)?;

            let input = if let Some(pair) = pair {
                tk::tokenizer::EncodeInput::Dual(sentence, pair)
            } else {
                tk::tokenizer::EncodeInput::Single(sentence)
            };

            let worker = {
                let this = cx.this();
                let guard = cx.lock();
                let worker = this.borrow(&guard).prepare_for_task();
                worker
            };

            let task = EncodeTask::Single(worker, Some(input));
            task.schedule(callback);
            Ok(cx.undefined().upcast())
        }

        method encodeBatch(mut cx) {
            // type EncodeInput = (String | [String, String])[]
            // encode_batch(sentences: EncodeInput[], __callback: (err, encodings) -> void)
            let inputs = cx.argument::<JsArray>(0)?.to_vec(&mut cx)?;
            let inputs = inputs.into_iter().map(|value| {
                if let Ok(s) = value.downcast::<JsString>() {
                    Ok(tk::tokenizer::EncodeInput::Single(s.value()))
                } else if let Ok(arr) = value.downcast::<JsArray>() {
                    if arr.len() != 2 {
                        cx.throw_error("Input must be an array of `String | [String, String]`")
                    } else {
                        Ok(tk::tokenizer::EncodeInput::Dual(
                            arr.get(&mut cx, 0)?
                                .downcast::<JsString>()
                                .or_throw(&mut cx)?
                                .value(),
                            arr.get(&mut cx, 1)?
                                .downcast::<JsString>()
                                .or_throw(&mut cx)?
                                .value())
                        )
                    }
                } else {
                    cx.throw_error("Input must be an array of `String | [String, String]`")
                }
            }).collect::<NeonResult<Vec<_>>>()?;
            let callback = cx.argument::<JsFunction>(1)?;

            let worker = {
                let this = cx.this();
                let guard = cx.lock();
                let worker = this.borrow(&guard).prepare_for_task();
                worker
            };

            let task = EncodeTask::Batch(worker, Some(inputs));
            task.schedule(callback);
            Ok(cx.undefined().upcast())
        }

        method decode(mut cx) {
            // decode(ids: number[], skipSpecialTokens: bool = true)

            let ids = cx.argument::<JsArray>(0)?.to_vec(&mut cx)?
                .into_iter()
                .map(|id| {
                    id.downcast::<JsNumber>()
                        .or_throw(&mut cx)
                        .map(|v| v.value() as u32)
                })
                .collect::<NeonResult<Vec<_>>>()?;
            let mut skip_special_tokens = true;
            if let Ok(skip) = cx.argument::<JsBoolean>(1) {
                skip_special_tokens = skip.value();
            }

            let this = cx.this();
            let guard = cx.lock();
            let res = this.borrow(&guard).tokenizer.decode(ids, skip_special_tokens);
            let s = res.map_err(|e| cx.throw_error::<_, ()>(format!("{}", e)).unwrap_err())?;

            Ok(cx.string(s).upcast())
        }

        method decodeBatch(mut cx) {
            // decodeBatch(sequences: number[][], skipSpecialTokens: bool = true)

            let sentences = cx.argument::<JsArray>(0)?
                .to_vec(&mut cx)?
                .into_iter()
                .map(|sentence| {
                    sentence.downcast::<JsArray>()
                        .or_throw(&mut cx)?
                        .to_vec(&mut cx)?
                        .into_iter()
                        .map(|id| {
                            id.downcast::<JsNumber>()
                                .or_throw(&mut cx)
                                .map(|v| v.value() as u32)
                        })
                        .collect::<NeonResult<Vec<_>>>()
                }).collect::<NeonResult<Vec<_>>>()?;

            let mut skip_special_tokens = true;
            if let Ok(skip) = cx.argument::<JsBoolean>(1) {
                skip_special_tokens = skip.value();
            }

            let this = cx.this();
            let guard = cx.lock();
            let res = this.borrow(&guard).tokenizer.decode_batch(sentences, skip_special_tokens);
            let sentences = res
                .map_err(|e| cx.throw_error::<_, ()>(format!("{}", e)).unwrap_err())?;

            let js_sentences = JsArray::new(&mut cx, sentences.len() as u32);
            for (i, sentence) in sentences.into_iter().enumerate() {
                let s = cx.string(sentence);
                js_sentences.set(&mut cx, i as u32, s)?;
            }

            Ok(js_sentences.upcast())
        }

        method tokenToId(mut cx) {
            // tokenToId(token: string): number | undefined

            let token = cx.argument::<JsString>(0)?.value();

            let this = cx.this();
            let guard = cx.lock();
            let id = this.borrow(&guard).tokenizer.token_to_id(&token);

            if let Some(id) = id {
                Ok(cx.number(id).upcast())
            } else {
                Ok(cx.undefined().upcast())
            }
        }

        method idToToken(mut cx) {
            // idToToken(id: number): string | undefined

            let id = cx.argument::<JsNumber>(0)?.value() as u32;

            let this = cx.this();
            let guard = cx.lock();
            let token = this.borrow(&guard).tokenizer.id_to_token(id);

            if let Some(token) = token {
                Ok(cx.string(token).upcast())
            } else {
                Ok(cx.undefined().upcast())
            }
        }

        method addTokens(mut cx) {
            // addTokens(tokens: (string | [string, bool])[]): number

            let tokens = cx.argument::<JsArray>(0)?
                .to_vec(&mut cx)?
                .into_iter()
                .map(|token| {
                    if let Ok(token) = token.downcast::<JsString>() {
                        Ok(tk::tokenizer::AddedToken {
                            content: token.value(),
                            ..Default::default()
                        })
                    } else if let Ok(tuple) = token.downcast::<JsArray>() {
                        let token = tuple.get(&mut cx, 0)?
                            .downcast::<JsString>()
                            .or_throw(&mut cx)?
                            .value();
                        let word = tuple.get(&mut cx, 1)?
                            .downcast::<JsBoolean>()
                            .or_throw(&mut cx)?
                            .value();

                        Ok(tk::tokenizer::AddedToken {
                            content: token,
                            single_word: word,
                        })
                    } else {
                        cx.throw_error("Input must be `(string | [string, bool])[]`")
                    }
                })
                .collect::<NeonResult<Vec<_>>>()?;

            let mut this = cx.this();
            let guard = cx.lock();
            let added = this.borrow_mut(&guard).tokenizer.add_tokens(&tokens);

            Ok(cx.number(added as f64).upcast())
        }

        method addSpecialTokens(mut cx) {
            // addSpecialTokens(tokens: string[]): number

            let tokens = cx.argument::<JsArray>(0)?
                .to_vec(&mut cx)?
                .into_iter()
                .map(|token| {
                    Ok(token.downcast::<JsString>().or_throw(&mut cx)?.value())
                })
                .collect::<NeonResult<Vec<_>>>()?;

            let mut this = cx.this();
            let guard = cx.lock();
            let added = this.borrow_mut(&guard)
                .tokenizer
                .add_special_tokens(&tokens
                    .iter()
                    .map(|s| &s[..])
                    .collect::<Vec<_>>()
            );

            Ok(cx.number(added as f64).upcast())
        }

        method train(mut cx) {
            // train(trainer: JsTrainer, files: string[])

            let trainer = cx.argument::<JsTrainer>(0)?;
            let files = cx.argument::<JsArray>(1)?.to_vec(&mut cx)?.into_iter().map(|file| {
                Ok(file.downcast::<JsString>().or_throw(&mut cx)?.value())
            }).collect::<NeonResult<Vec<_>>>()?;

            let mut this = cx.this();
            let guard = cx.lock();
            let res = trainer.borrow(&guard).trainer.execute(|trainer| {
                let res = this.borrow_mut(&guard).tokenizer.train(trainer.unwrap(), files);
                res
            });
            res.map_err(|e| cx.throw_error::<_, ()>(format!("{}", e)).unwrap_err())?;

            Ok(cx.undefined().upcast())
        }

        method getModel(mut cx) {
            // getModel(): Model
            unimplemented!()
        }

        method setModel(mut cx) {
            // setModel(model: JsModel)

            let running = {
                let this = cx.this();
                let guard = cx.lock();
                let count = std::sync::Arc::strong_count(&this.borrow(&guard).running_task);
                count
            };
            if running > 1 {
                println!("{} running tasks", running - 1);
                return cx.throw_error("Cannot modify the tokenizer while there are running tasks");
            }

            let mut model = cx.argument::<JsModel>(0)?;
            if let Some(instance) = {
                let guard = cx.lock();
                let mut model = model.borrow_mut(&guard);
                model.model.to_pointer()
            } {
                let mut this = cx.this();
                {
                    let guard = cx.lock();
                    let mut tokenizer = this.borrow_mut(&guard);
                    tokenizer.tokenizer.with_model(instance);
                }

                Ok(cx.undefined().upcast())
            } else {
                cx.throw_error("The Model is already being used in another Tokenizer")
            }
        }

        method getNormalizer(mut cx) {
            // getNormalizer(): Normalizer | undefined
            unimplemented!()
        }

        method setNormalizer(mut cx) {
            // setNormalizer(normalizer: Normalizer)
            unimplemented!()
        }

        method getPreTokenizer(mut cx) {
            // getPreTokenizer(): PreTokenizer | undefined
            unimplemented!()
        }

        method setPreTokenizer(mut cx) {
            // setPreTokenizer(pretokenizer: PreTokenizer)
            unimplemented!()
        }

        method getPostProcessor(mut cx) {
            // getPostProcessor(): PostProcessor | undefined
            unimplemented!()
        }

        method setPostProcessor(mut cx) {
            // setPostProcessor(processor: PostProcessor)
            unimplemented!()
        }

        method getDecoder(mut cx) {
            // getDecoder(): Decoder | undefined
            unimplemented!()
        }

        method setDecoder(mut cx) {
            // setDecoder(decoder: Decoder)
            unimplemented!()
        }
    }
}

pub fn register(m: &mut ModuleContext, prefix: &str) -> Result<(), neon::result::Throw> {
    m.export_class::<JsTokenizer>(&format!("{}_Tokenizer", prefix))?;
    Ok(())
}
