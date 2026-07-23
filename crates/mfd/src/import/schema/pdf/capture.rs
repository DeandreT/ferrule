use std::collections::BTreeSet;

use mapping::{PdfCapture, PdfCaptureAlgorithm, PdfWhitespaceMode, PdfWordSeparation};

use super::{parse_required_region, required_text_child};

pub(super) fn parse(node: &roxmltree::Node<'_, '_>) -> Result<PdfCapture, String> {
    let algorithms = node
        .children()
        .filter(|child| child.has_tag_name("Algorithm"))
        .collect::<Vec<_>>();
    let algorithm = match algorithms.as_slice() {
        [] => PdfCaptureAlgorithm::default(),
        [algorithm] => parse_algorithm(algorithm)?,
        _ => return Err("PDF Capture declares more than one Algorithm policy".to_string()),
    };
    Ok(PdfCapture {
        name: required_text_child(node, "Label")?,
        region: parse_required_region(node)?,
        algorithm,
    })
}

fn parse_algorithm(algorithm: &roxmltree::Node<'_, '_>) -> Result<PdfCaptureAlgorithm, String> {
    let choices = algorithm
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [basic_visual] = choices.as_slice() else {
        return Err(
            "PDF Capture Algorithm must contain exactly one <BasicVisual> policy".to_string(),
        );
    };
    if !basic_visual.has_tag_name("BasicVisual") {
        return Err(format!(
            "PDF Capture Algorithm <{}> is unsupported; only <BasicVisual> is supported",
            basic_visual.tag_name().name()
        ));
    }

    let mut seen = BTreeSet::new();
    let mut has_word_separation = false;
    for option in basic_visual.children().filter(roxmltree::Node::is_element) {
        let name = option.tag_name().name();
        if !seen.insert(name) {
            return Err(format!(
                "PDF BasicVisual capture option <{name}> is declared more than once"
            ));
        }
        match name {
            "BaselineCapture" | "ParagraphSpacing" | "BaselineAngle" | "AngleDeviation" => {
                require_empty_option(&option)?;
            }
            "SeparateWords" => {
                require_choice(&option, "InsertSpace")?;
                has_word_separation = true;
            }
            "WhitespaceMode" => require_choice(&option, "Default")?,
            other => {
                return Err(format!(
                    "PDF BasicVisual capture option <{other}> is unsupported; supported text reconstruction uses <SeparateWords><InsertSpace/></SeparateWords> and <WhitespaceMode><Default/></WhitespaceMode>"
                ));
            }
        }
    }
    if !has_word_separation {
        return Err(
            "PDF BasicVisual capture requires <SeparateWords><InsertSpace/></SeparateWords>"
                .to_string(),
        );
    }
    Ok(PdfCaptureAlgorithm::BasicVisual {
        separate_words: PdfWordSeparation::InsertSpace,
        whitespace: PdfWhitespaceMode::Default,
    })
}

fn require_empty_option(node: &roxmltree::Node<'_, '_>) -> Result<(), String> {
    if node
        .children()
        .any(|child| child.is_element() || child.text().is_some_and(|text| !text.trim().is_empty()))
    {
        Err(format!(
            "PDF BasicVisual capture option <{}> supports only its empty default",
            node.tag_name().name()
        ))
    } else {
        Ok(())
    }
}

fn require_choice(node: &roxmltree::Node<'_, '_>, expected: &str) -> Result<(), String> {
    let choices = node
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    match choices.as_slice() {
        [choice] if choice.has_tag_name(expected) => Ok(()),
        [choice] => Err(format!(
            "PDF BasicVisual capture option <{}><{}/></{}> is unsupported; expected <{expected}/>",
            node.tag_name().name(),
            choice.tag_name().name(),
            node.tag_name().name()
        )),
        _ => Err(format!(
            "PDF BasicVisual capture option <{}> must contain exactly one <{expected}/> mode",
            node.tag_name().name()
        )),
    }
}

#[cfg(test)]
mod tests {
    use mapping::{PdfCaptureAlgorithm, PdfWhitespaceMode, PdfWordSeparation};

    use super::parse_algorithm;

    #[test]
    fn policy_is_typed_and_unknown_modes_are_actionable() {
        let Ok(supported) = roxmltree::Document::parse(
            "<Algorithm><BasicVisual><BaselineCapture/><ParagraphSpacing/><BaselineAngle/><AngleDeviation/><SeparateWords><InsertSpace/></SeparateWords><WhitespaceMode><Default/></WhitespaceMode></BasicVisual></Algorithm>",
        ) else {
            panic!("supported BasicVisual policy XML must parse");
        };
        assert_eq!(
            parse_algorithm(&supported.root_element()),
            Ok(PdfCaptureAlgorithm::BasicVisual {
                separate_words: PdfWordSeparation::InsertSpace,
                whitespace: PdfWhitespaceMode::Default,
            })
        );

        let Ok(unsupported) = roxmltree::Document::parse(
            "<Algorithm><BasicVisual><SeparateWords><KeepAdjacent/></SeparateWords></BasicVisual></Algorithm>",
        ) else {
            panic!("unsupported BasicVisual policy XML must still parse");
        };
        assert!(matches!(
            parse_algorithm(&unsupported.root_element()),
            Err(message)
                if message.contains("KeepAdjacent")
                    && message.contains("expected <InsertSpace/>")
        ));

        let Ok(alternative) =
            roxmltree::Document::parse("<Algorithm><OpticalCharacterRecognition/></Algorithm>")
        else {
            panic!("unsupported capture algorithm XML must still parse");
        };
        assert!(matches!(
            parse_algorithm(&alternative.root_element()),
            Err(message)
                if message.contains("OpticalCharacterRecognition")
                    && message.contains("only <BasicVisual>")
        ));
    }
}
