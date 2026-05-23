import { useState } from 'react';
import { Autocomplete, Chip, TextField } from '@mui/material';

interface LabelSelectorProps {
  value: string[];
  onChange: (labels: string[]) => void;
  disabled?: boolean;
}

export default function LabelSelector({
  value,
  onChange,
  disabled,
}: LabelSelectorProps) {
  const [inputValue, setInputValue] = useState('');

  const parseLabels = (rawValue: string) =>
    rawValue
      .split(',')
      .map((label) => label.trim())
      .filter((label) => label.length > 0);

  const normalizeLabels = (labels: string[]) =>
    labels
      .flatMap(parseLabels)
      .filter((label, index, labels) => labels.indexOf(label) === index);

  const commitInputValue = () => {
    const parsedInput = parseLabels(inputValue);
    if (parsedInput.length === 0) return;

    onChange(normalizeLabels([...value, ...parsedInput]));
    setInputValue('');
  };

  return (
    <Autocomplete
      multiple
      freeSolo
      options={[]}
      value={value}
      inputValue={inputValue}
      onInputChange={(_, newInputValue) => {
        setInputValue(newInputValue);
      }}
      onChange={(_, newValue) => {
        onChange(normalizeLabels(newValue as string[]));
        setInputValue('');
      }}
      disabled={disabled}
      renderTags={(tagValue, getTagProps) =>
        tagValue.map((option, index) => {
          const { key, ...tagProps } = getTagProps({ index });
          return (
            <Chip
              key={key}
              label={option}
              size="small"
              {...tagProps}
              sx={{
                backgroundColor: '#ededf9',
                color: '#191b23',
                borderRadius: '4px',
                fontWeight: 500,
                fontSize: '12px',
              }}
            />
          );
        })
      }
      renderInput={(params) => (
        <TextField
          {...params}
          label="标签"
          placeholder="输入标签后按回车添加"
          helperText="输入标签名后按 Enter 添加，需为仓库中已存在的标签"
          onBlur={commitInputValue}
        />
      )}
    />
  );
}
